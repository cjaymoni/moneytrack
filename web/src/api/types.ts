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
