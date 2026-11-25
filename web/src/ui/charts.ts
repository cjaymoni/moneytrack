// Labels for the charts page. Text in, text out: the amounts and every height on a chart are
// `mt-core`'s, and nothing here measures money.

import type { Money } from "../api/types";
import { balance } from "./format";

/**
 * A gridline's amount, short: `"2000.00"` → `"2,000"`.
 *
 * The scale only ever steps by whole units of the currency, so its trailing zeros say
 * nothing; dropping them is trimming text, not rounding. Anything else is kept as it came.
 */
export function tickLabel(amount: Money): string {
  return balance(amount).replace(/\.0+$/, "");
}

/**
 * A month on a chart's axis: "Nov", or "Jan 2027" where the year needs saying — on the
 * first month shown and on each January after it.
 */
export function axisMonth(month: string, isFirst: boolean): string {
  const [year = 1970, number = 1] = month.split("-").map(Number);
  const withYear = isFirst || number === 1;
  return new Intl.DateTimeFormat("en-GB", {
    month: "short",
    ...(withYear ? { year: "numeric" } : {}),
  }).format(new Date(year, number - 1, 1));
}

/** Days of the month that get a label on the axis; the rest stay blank so labels never crowd. */
const LABELLED_DAYS = new Set([1, 5, 10, 15, 20, 25, 30]);

/** The day number on a chart's axis for a `YYYY-MM-DD` date, or "" for an unlabelled day. */
export function axisDay(date: string): string {
  const day = Number(date.slice(8, 10));
  return LABELLED_DAYS.has(day) ? String(day) : "";
}

/** "Tuesday 3 November", read as the date it says on the reader's own calendar. */
export function dayName(date: string): string {
  const [year = 1970, month = 1, day = 1] = date.split("-").map(Number);
  return new Intl.DateTimeFormat("en-GB", { weekday: "long", day: "numeric", month: "long" }).format(
    new Date(year, month - 1, day),
  );
}
