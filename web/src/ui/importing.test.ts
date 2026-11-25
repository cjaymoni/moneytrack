import { describe, expect, it } from "vitest";
import { columnLetter, formatOf, guessMapping, toBase64 } from "./importing";

describe("toBase64", () => {
  it("encodes bytes as they are, including ones that are not UTF-8", () => {
    expect(toBase64(new TextEncoder().encode("Date,Amount\n"))).toBe(btoa("Date,Amount\n"));
    expect(toBase64(new Uint8Array([0x43, 0x61, 0x66, 0xe9]))).toBe("Q2Fm6Q==");
    expect(toBase64(new Uint8Array([]))).toBe("");
  });

  it("encodes a file larger than one slice", () => {
    const big = new Uint8Array(100_000).fill(0x61);
    expect(atob(toBase64(big))).toHaveLength(100_000);
  });
});

describe("formatOf", () => {
  it("reads OFX from either extension and anything else as CSV", () => {
    expect(formatOf("nov.ofx")).toBe("ofx");
    expect(formatOf("NOV.QFX")).toBe("ofx");
    expect(formatOf("nov.csv")).toBe("csv");
    expect(formatOf("statement.txt")).toBe("csv");
  });
});

describe("guessMapping", () => {
  it("finds a header below an account summary and reads a signed amount off it", () => {
    const mapping = guessMapping(
      [
        ["Account", "0012345"],
        ["Date", "Description", "Amount", "Balance"],
        ["03/11/2026", "SHOPRITE", "-20.00", "100.00"],
      ],
      "comma",
    );

    expect(mapping).toMatchObject({
      skip_rows: 1,
      has_header: true,
      date: 0,
      date_format: "day_month_year",
      label: 1,
      amount: { kind: "signed", column: 2 },
      balance: 3,
    });
  });

  it("recognises separate money-in and money-out columns", () => {
    const mapping = guessMapping(
      [
        ["Value Date", "Narration", "Debit", "Credit", "Reference"],
        ["2026-11-03", "ATM", "50.00", "", "R1"],
      ],
      "comma",
    );

    expect(mapping.amount).toEqual({ kind: "split", money_in: 3, money_out: 2 });
    expect(mapping.date_format).toBe("year_month_day");
    expect(mapping.reference).toBe(4);
  });

  it("assumes date, description, amount when nothing names a date", () => {
    const mapping = guessMapping([["03/11/2026", "Fare", "-5"]], "semicolon");

    expect(mapping).toMatchObject({
      delimiter: "semicolon",
      has_header: false,
      skip_rows: 0,
      date: 0,
      label: 1,
      amount: { kind: "signed", column: 2 },
    });
  });
});

describe("columnLetter", () => {
  it("names columns as a spreadsheet does", () => {
    expect([0, 1, 25, 26, 27].map(columnLetter)).toEqual(["A", "B", "Z", "AA", "AB"]);
  });
});
