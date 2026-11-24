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
  /** The repeating series that wrote it, if any. */
  recurring_id: string | null;
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

/** One month's money in and out, and where each sits on its trend's scale. */
export interface TrendMonth {
  /** `YYYY-MM`. */
  month: string;
  starts_on: string;
  ends_on: string;
  income: Money;
  /** Positive: money that left. */
  spending: Money;
  net: Money;
  /** Height on the trend's scale, 0 to 10 000, worked out by `mt-core`. For drawing only. */
  income_level_bps: number;
  spending_level_bps: number;
}

/** A gridline on a trend's scale. */
export interface Tick {
  amount: Money;
  level_bps: number;
}

/** Several months in one currency, on one scale. Never converted into another currency. */
export interface Trend {
  currency: string;
  /** Oldest first. */
  months: TrendMonth[];
  /** From zero up to the top of the scale. */
  ticks: Tick[];
}

/** What left on one date, and its height on the month's scale. */
export interface DaySpend {
  /** `YYYY-MM-DD`, on the reader's clock. */
  date: string;
  /** Positive: money that left. */
  spending: Money;
  /** 0 to 10 000, worked out by `mt-core`. For drawing only. */
  level_bps: number;
}

/** One month's spending in one currency, day by day, on one scale. */
export interface Daily {
  currency: string;
  starts_on: string;
  ends_on: string;
  total: Money;
  /** Every date in the month, in order, days with nothing spent included. */
  days: DaySpend[];
  ticks: Tick[];
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

export type BudgetPeriod = "weekly" | "monthly" | "yearly";

/** Where a budget stands on one basis, as `mt-core` worked it out. */
export interface Standing {
  period_starts_on: string;
  period_ends_on: string;
  spent: Money;
  remaining: Money;
  /** Positive, once past the cap. */
  over_by: Money | null;
  /** Basis points of the cap; past 10 000 once over. */
  used_bps: number;
  used_display: string;
  level: "under" | "warning" | "over";
}

export interface Budget {
  id: string;
  category_id: string | null;
  name: string;
  period: BudgetPeriod;
  cap: Money;
  warn_at_bps: number;
  starts_on: string;
  ends_on: string | null;
  /** Counting what has happened. Absent when it is not running today. */
  now: Standing | null;
  /** Counting this period's expected rows too. */
  expected: Standing | null;
}

export interface NewBudget {
  category_id?: string;
  period: BudgetPeriod;
  /** The decimal text a person typed. */
  cap: string;
  currency: string;
  warn_at_bps?: number;
}

/** The answer to counting an account. */
export interface CountResult {
  /** What the ledger said the account held when it was counted. */
  ledger: Money;
  counted: Money;
  /** The correcting row written; absent when the count agreed. */
  adjustment: Transaction | null;
}

export type Frequency = "daily" | "weekly" | "monthly" | "yearly";

/** A repeating series, and the next day it falls on. */
export interface Recurring {
  id: string;
  account_id: string;
  account_name: string;
  direction: Direction;
  amount: Money;
  label: string;
  counterparty: string | null;
  category_id: string | null;
  freq: Frequency;
  interval: number;
  starts_on: string;
  ends_on: string | null;
  /** Absent once the series has ended. */
  next_on: string | null;
}

export interface NewRecurring {
  account_id: string;
  direction: Direction;
  /** The decimal text the person typed. */
  amount: string;
  label: string;
  counterparty?: string;
  category_id?: string;
  freq: Frequency;
  interval: number;
  starts_on: string;
  ends_on?: string;
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

// ---------------------------------------------------------------------------- imports ----

export type Delimiter = "comma" | "semicolon" | "tab" | "pipe";
export type DateOrder = "day_month_year" | "month_day_year" | "year_month_day";
export type ImportFormat = "csv" | "ofx";

/** Where the amount is in a CSV statement. Columns count from 0. */
export type AmountColumns =
  | { kind: "signed"; column: number }
  | { kind: "split"; money_in: number; money_out: number };

/**
 * Which column of a CSV statement holds what. The shape is `mt-core`'s, sent and stored as
 * it is; the server validates it.
 */
export interface CsvMapping {
  delimiter: Delimiter;
  skip_rows: number;
  has_header: boolean;
  date: number;
  date_format: DateOrder;
  amount: AmountColumns;
  label: number;
  counterparty: number | null;
  reference: number | null;
  balance: number | null;
}

export interface ImportPreview {
  delimiter: Delimiter;
  /** The first rows of the file, split into cells. */
  rows: string[][];
  /** The mapping last used for this account, if any. */
  saved_mapping: CsvMapping | null;
}

export interface ImportOutcome {
  import_id: string;
  read: number;
  added: number;
  already_seen: number;
  problem_count: number;
  problems: { line_no: number; reason: string }[];
}

export interface Suspect {
  transaction: Transaction;
  score_bps: number;
  /** Both carry the bank's own reference. Still offered, never merged without the person. */
  certain: boolean;
  signals: string[];
}

/** A statement line waiting for a person. */
export interface InboxItem {
  id: string;
  source: ImportFormat;
  file_name: string | null;
  line_no: number;
  account_id: string;
  account_name: string;
  direction: Direction;
  /** Always positive; `direction` carries the sign. */
  amount: Money;
  booked_at: string;
  label: string;
  counterparty: string | null;
  /** What the server's categorisation ladder proposed. */
  category_id: string | null;
  category_confidence_bps: number;
  reported_balance: Money | null;
  suspects: Suspect[];
  /** Waiting lines from other uploads that look like the same movement. */
  twins: { id: string; file_name: string | null; line_no: number; label: string }[];
}

/** How many lines wait, without reading them. */
export interface InboxCount {
  total: number;
  by_account: { account_id: string; waiting: number }[];
}

export interface Inbox {
  /** Every waiting line, including any not in `items`. */
  total: number;
  items: InboxItem[];
}
