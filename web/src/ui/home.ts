import type { Budget, Money, Outlook, Transaction } from "../api/types";

/** A planned payment, with the account it comes out of or goes into. */
export interface Upcoming {
  transaction: Transaction;
  accountName: string;
}

/**
 * The next planned payments across every account, soonest first.
 *
 * Each outlook already lists its own account's planned rows in date order; this only merges
 * them. Ties fall back to the label so the order is the same on every reload.
 */
export function upcoming(outlooks: Outlook[], limit: number): Upcoming[] {
  const all = outlooks.flatMap((outlook) =>
    outlook.points.map((point) => ({ transaction: point.transaction, accountName: outlook.account.name })),
  );
  all.sort(
    (left, right) =>
      left.transaction.booked_at.localeCompare(right.transaction.booked_at) ||
      left.transaction.label.localeCompare(right.transaction.label),
  );
  return all.slice(0, Math.max(0, limit));
}

/** An account that is below zero now, or will go below zero within the forecast. */
export interface Shortfall {
  accountId: string;
  accountName: string;
  currency: string;
  /** Below zero already, or going below on `on` because of `cause`. */
  already: boolean;
  on: string | null;
  cause: string | null;
  /** What the account will hold: now if already below zero, else after `cause`. */
  low: Money;
}

/**
 * Every account the forecast says goes below zero, as one sentence's worth each.
 *
 * The server decides whether and where: an outlook's `shortfall` and the row that causes it.
 * A shortfall the server reports without naming a row it can be matched to is still shown,
 * with no date, rather than dropped.
 */
export function shortfalls(outlooks: Outlook[]): Shortfall[] {
  return outlooks.flatMap((outlook): Shortfall[] => {
    const { account } = outlook;
    const base = { accountId: account.id, accountName: account.name, currency: account.currency };
    if (outlook.shortfall === "already") {
      return [{ ...base, already: true, on: null, cause: null, low: outlook.start }];
    }
    if (outlook.shortfall === "at") {
      const point = outlook.points.find((each) => each.transaction.id === outlook.shortfall_transaction_id);
      return [
        {
          ...base,
          already: false,
          on: point?.transaction.booked_at ?? null,
          cause: point?.transaction.label ?? null,
          low: point?.balance_after ?? outlook.low,
        },
      ];
    }
    return [];
  });
}

/**
 * Budgets near or past their limit this period, the worst first.
 *
 * "Near" is the server's call: each budget's warning level is its own setting, and the
 * server has already compared against it.
 */
export function budgetsToWatch(budgets: Budget[]): Budget[] {
  const severity = { over: 0, warning: 1, under: 2 } as const;
  return budgets
    .filter((budget) => budget.now && budget.now.level !== "under")
    .sort((left, right) => {
      const a = left.now;
      const b = right.now;
      if (!a || !b) return 0;
      return severity[a.level] - severity[b.level] || b.used_bps - a.used_bps;
    });
}
