// What crosses the wire, spelled as `crates/server/src/api/dto.rs` spells it.
//
// Every amount arrives with a `display` string `mt-core` rendered. That string is what the
// screens print. `minor_units` is here because it is on the wire, and it is used for one
// thing only: asking whether an amount is below zero, which is a sign, not arithmetic.

export interface Money {
  /** Signed, in minor units. Never divided by anything in this client. */
  minor_units: number;
  /** ISO 4217 code. */
  currency: string;
  /** The amount at the currency's own decimal places, rendered by `mt-core`. */
  display: string;
}

export interface User {
  id: string;
  email: string;
  created_at: string;
}

export type AccountKind = "cash" | "momo" | "bank" | "card";

export interface Account {
  id: string;
  name: string;
  kind: AccountKind;
  currency: string;
  opening_balance: Money;
  /** Counting only rows known to have happened. */
  confirmed: Money;
  /** Counting the rows still expected too — the forecast. */
  projected: Money;
}

export type Direction = "in" | "out";
export type Status = "pending" | "confirmed";

export interface Transaction {
  id: string;
  account_id: string;
  direction: Direction;
  /** Always positive; `direction` carries the sign. */
  amount: Money;
  booked_at: string;
  category_id: string | null;
  label: string;
  counterparty: string | null;
  note: string | null;
  status: Status;
  source: string;
}

export interface LedgerEntry {
  transaction: Transaction;
  /** What the account held once this row was applied, walked by `mt-core`. */
  balance_after: Money;
}

export interface Ledger {
  account: Account;
  basis: "confirmed" | "projected";
  opening: Money;
  entries: LedgerEntry[];
  closing: Money;
}

export type CategoryKind = "income" | "expense";

export interface Category {
  id: string;
  name: string;
  /** The category it sits under. Nesting is one level. */
  parent_id: string | null;
  kind: CategoryKind;
  sort_order: number;
}

export interface NewCategory {
  name: string;
  kind: CategoryKind;
  parent_id?: string;
}

/** A share of one side of a breakdown, worked out by `mt-core`. */
export interface Share {
  /** 0 to 10 000. Used for the width of a bar and nothing else. */
  basis_points: number;
  /** For a person to read: "23%", or "<1%". */
  display: string;
}

export interface BreakdownPart {
  category_id: string;
  name: string;
  total: Money;
  share: Share;
}

export interface BreakdownLine {
  /** Absent for rows filed under no category. */
  category_id: string | null;
  name: string | null;
  /** Everything in it, children included. */
  total: Money;
  share: Share;
  /** Filed under this category itself rather than one of its children. */
  own: Money;
  own_share: Share;
  children: BreakdownPart[];
}

export interface BreakdownSide {
  total: Money;
  lines: BreakdownLine[];
}

/** One month in one currency. Never converted into another. */
export interface Breakdown {
  currency: string;
  starts_on: string;
  ends_on: string;
  spending: BreakdownSide;
  income: BreakdownSide;
  net: Money;
}

export interface OutlookPoint {
  transaction: Transaction;
  /** What the account holds once this row lands, walked by `mt-core`. */
  balance_after: Money;
  /** Height on the line, 0 to 10 000 on a scale that includes zero, worked out by `mt-core`. */
  level_bps: number;
}

/** One account from now to the end of the window. Never pooled with another account. */
export interface Outlook {
  account: Account;
  from: string;
  until: string;
  start: Money;
  start_level_bps: number;
  points: OutlookPoint[];
  low: Money;
  /** The row that brings the low; absent when the low is today. */
  low_transaction_id: string | null;
  high: Money;
  /** Where zero sits on the line. The scale always includes it. */
  zero_level_bps: number;
  shortfall: "none" | "already" | "at";
  shortfall_transaction_id: string | null;
}

export interface Currency {
  code: string;
  name: string;
}

export interface NewAccount {
  name: string;
  kind: AccountKind;
  currency: string;
  /** The decimal text the person typed. The server reads it in the account's currency. */
  opening_balance?: string;
}

/** A row as a person can change it: all of it, sent whole. */
export interface TransactionEdit {
  direction: Direction;
  /** The decimal text the person typed. */
  amount: string;
  /** RFC 3339. Required: an edit never moves a row to now by leaving the date out. */
  booked_at: string;
  label: string;
  counterparty: string | null;
  note: string | null;
  status: Status;
  category_id: string | null;
}

export interface NewTransaction {
  account_id: string;
  direction: Direction;
  /** The decimal text the person typed. The server reads it in the account's currency. */
  amount: string;
  label: string;
  counterparty?: string;
  note?: string;
  /** RFC 3339. Absent means now. */
  booked_at?: string;
  status?: Status;
  category_id?: string;
}
