// Getting a statement file to the server, and a first guess at which column is which.
//
// Nothing here reads an amount or a date. The file goes up as bytes and `mt-core` reads it;
// the guess below only picks which column to offer first in a form the person confirms.

import type { CsvMapping, Delimiter, ImportFormat } from "../api/types";

/** How much of a file the column preview needs: the first rows, not the year. */
export const PREVIEW_BYTES = 64 * 1024;

/** The largest statement the server reads. Checked here only to say so before uploading. */
export const MAX_STATEMENT_BYTES = 5 * 1024 * 1024;

/** How many rows are searched for the one that names the columns. */
const HEADER_SEARCH_ROWS = 10;

/** `btoa` takes a string of byte-sized characters; building it in slices keeps a 5 MB file
 * from becoming one enormous argument list. */
const BASE64_SLICE = 0x8000;

/** Bytes as base64, exactly as they are: the server decides the text encoding, not us. */
export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let at = 0; at < bytes.length; at += BASE64_SLICE) {
    binary += String.fromCharCode(...bytes.subarray(at, at + BASE64_SLICE));
  }
  return btoa(binary);
}

/** A file, or its first `limit` bytes, as base64. */
export async function fileToBase64(file: Blob, limit?: number): Promise<string> {
  const part = limit === undefined ? file : file.slice(0, limit);
  return toBase64(new Uint8Array(await part.arrayBuffer()));
}

/** Which reader a file needs, from its name. OFX exports also come as `.qfx`. */
export function formatOf(fileName: string): ImportFormat {
  return /\.(ofx|qfx)$/i.test(fileName.trim()) ? "ofx" : "csv";
}

const HEADER_WORDS = {
  date: /date|posted|value/i,
  label: /desc|narrat|detail|particular|memo|remark|transaction/i,
  amount: /^\s*amount/i,
  moneyOut: /debit|withdraw|money out|paid out|dr\b/i,
  moneyIn: /credit|deposit|money in|paid in|cr\b/i,
  counterparty: /payee|counterparty|beneficiar|merchant|recipient|sender|\bname\b/i,
  reference: /ref|reference|\bid\b|cheque/i,
  balance: /balance/i,
};

/** The first column whose header matches, skipping columns already taken. */
function find(header: string[], pattern: RegExp, taken: Set<number>): number | null {
  const at = header.findIndex((cell, index) => !taken.has(index) && pattern.test(cell));
  return at === -1 ? null : at;
}

/**
 * A first guess at a mapping from the start of a file, for the person to confirm.
 *
 * Looks for the row that names a date column and reads the others off it; with no such
 * row, assumes there is no header and the columns run date, description, amount.
 */
export function guessMapping(rows: string[][], delimiter: Delimiter): CsvMapping {
  const headerAt = rows
    .slice(0, HEADER_SEARCH_ROWS)
    .findIndex((row) => row.some((cell) => HEADER_WORDS.date.test(cell)));
  if (headerAt === -1) {
    return {
      delimiter,
      skip_rows: 0,
      has_header: false,
      date: 0,
      date_format: dateOrderOf(rows[0]?.[0] ?? ""),
      amount: { kind: "signed", column: 2 },
      label: 1,
      counterparty: null,
      reference: null,
      balance: null,
    };
  }

  const header = rows[headerAt] ?? [];
  const taken = new Set<number>();
  const claim = (pattern: RegExp): number | null => {
    const at = find(header, pattern, taken);
    if (at !== null) taken.add(at);
    return at;
  };

  const date = claim(HEADER_WORDS.date) ?? 0;
  const balance = claim(HEADER_WORDS.balance);
  const signed = claim(HEADER_WORDS.amount);
  const moneyOut = signed === null ? claim(HEADER_WORDS.moneyOut) : null;
  const moneyIn = signed === null ? claim(HEADER_WORDS.moneyIn) : null;
  const label = claim(HEADER_WORDS.label);
  const counterparty = claim(HEADER_WORDS.counterparty);
  const reference = claim(HEADER_WORDS.reference);
  const firstFree = header.findIndex((_, index) => !taken.has(index));

  const amount =
    moneyIn !== null && moneyOut !== null
      ? ({ kind: "split", money_in: moneyIn, money_out: moneyOut } as const)
      : ({ kind: "signed", column: signed ?? moneyOut ?? moneyIn ?? Math.max(firstFree, 0) } as const);

  return {
    delimiter,
    skip_rows: headerAt,
    has_header: true,
    date,
    date_format: dateOrderOf(rows[headerAt + 1]?.[date] ?? ""),
    amount,
    label: label ?? counterparty ?? Math.max(firstFree, 0),
    counterparty,
    reference,
    balance,
  };
}

/**
 * The order a date cell is probably in. A leading four-digit year is unambiguous; anything
 * else is taken as day first, which is how Ghana writes dates. The person can change it.
 */
function dateOrderOf(cell: string): CsvMapping["date_format"] {
  return /^\s*\d{4}[-/.]/.test(cell) ? "year_month_day" : "day_month_year";
}

/** A column's letter, as a spreadsheet would show it: 0 → A, 26 → AA. */
export function columnLetter(index: number): string {
  let letters = "";
  let n = index + 1;
  while (n > 0) {
    const rem = (n - 1) % 26;
    letters = String.fromCharCode(65 + rem) + letters;
    n = Math.floor((n - 1) / 26);
  }
  return letters;
}
