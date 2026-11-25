// Grouping uncategorised rows by who they were with, so a payee can be filed once.
// Text only: amounts are shown as the server rendered them and never added up here.

import type { Direction, Transaction } from "../api/types";

/** The uncategorised rows from one payee, one way. */
export interface PayeeGroup {
  /** Stable across reloads: direction plus the payee as compared. */
  key: string;
  /** The payee as written on the first row. */
  payee: string;
  /** Which field the payee came from, so a rule can look in the same place next time. */
  field: "counterparty" | "label";
  direction: Direction;
  rows: Transaction[];
}

/** How two payee names are compared: case and surrounding space do not count. */
function compared(text: string): string {
  return text.trim().toLocaleLowerCase("en-GB");
}

/**
 * Every uncategorised row, grouped by payee and direction, the biggest groups first.
 *
 * The payee is the counterparty when the row has one, else its description — imported rows
 * often carry the merchant only in the description. Money in and money out from the same
 * name are separate groups: they are filed under different kinds of category.
 */
export function groupByPayee(rows: Transaction[]): PayeeGroup[] {
  const groups = new Map<string, PayeeGroup>();
  for (const row of rows) {
    if (row.category_id !== null) continue;
    const counterparty = row.counterparty?.trim();
    const payee = counterparty || row.label.trim();
    if (!payee) continue;
    const field = counterparty ? "counterparty" : "label";
    const key = `${row.direction}:${field}:${compared(payee)}`;
    const group = groups.get(key);
    if (group) {
      group.rows.push(row);
    } else {
      groups.set(key, { key, payee, field, direction: row.direction, rows: [row] });
    }
  }
  return [...groups.values()].sort(
    (left, right) =>
      right.rows.length - left.rows.length ||
      left.payee.localeCompare(right.payee) ||
      left.key.localeCompare(right.key),
  );
}
