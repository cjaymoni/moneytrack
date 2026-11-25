// Narrowing a ledger to what someone is looking for. Text and categories only: no amount is
// compared or added up here.

import type { LedgerEntry } from "../api/types";

/** Which rows to show. */
export type Show = "all" | "uncategorised" | "planned";

export interface LedgerFilter {
  /** Words to find in the description, payee or note. */
  text: string;
  show: Show;
}

export const NO_FILTER: LedgerFilter = { text: "", show: "all" };

/** Whether a filter narrows anything at all. */
export function isFiltering(filter: LedgerFilter): boolean {
  return filter.text.trim() !== "" || filter.show !== "all";
}

/**
 * Whether a row matches: every word typed appears somewhere in its description, payee,
 * note or category name, in any case, and it is the kind of row asked for.
 */
export function matches(entry: LedgerEntry, filter: LedgerFilter, categoryName?: string): boolean {
  const { transaction } = entry;
  if (filter.show === "uncategorised" && transaction.category_id !== null) return false;
  if (filter.show === "planned" && transaction.status !== "pending") return false;
  const words = filter.text.toLocaleLowerCase("en-GB").split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const haystack = [transaction.label, transaction.counterparty, transaction.note, categoryName]
    .filter(Boolean)
    .join(" ")
    .toLocaleLowerCase("en-GB");
  return words.every((word) => haystack.includes(word));
}
