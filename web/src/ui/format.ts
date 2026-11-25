// Presentation, and only presentation.
//
// Every function here takes text the server rendered and arranges it. None of them adds,
// scales or rounds an amount: `mt-core` decided where the decimal point goes, and this file
// only decides where the thousands commas go — which `mt-core` leaves to the screen on
// purpose ("symbols, grouping and locale are presentation concerns").

import type { Direction, Money } from "../api/types";

/** The real minus sign, so a negative figure lines up with the tabular digits beside it. */
const MINUS = "−";

/**
 * Groups the whole part of a rendered amount in threes: `"-12500.00"` → `"−12,500.00"`.
 *
 * Text in, text out. If the string is not the shape `mt-core` writes, it is returned as it
 * came rather than guessed at.
 */
export function grouped(display: string): string {
  const match = /^(-?)(\d+)(\.\d+)?$/.exec(display);
  if (!match) return display;
  const [, sign = "", whole = "", fraction = ""] = match;
  const withCommas = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return (sign ? MINUS : "") + withCommas + fraction;
}

/** A balance, grouped, carrying its own sign. */
export function balance(money: Money): string {
  return grouped(money.display);
}

/**
 * A row's amount with the sign its direction gives it: money in is `+`, money out is `−`.
 *
 * The amount on the wire is always positive and the direction says which way it went, so
 * this is a prefix chosen from a word — never a negation.
 */
export function signed(amount: Money, direction: Direction): string {
  return (direction === "in" ? "+" : MINUS) + grouped(amount.display);
}

/** Whether a balance is below zero. A sign read off the wire, not a computation. */
export function isNegative(money: Money): boolean {
  return money.minor_units < 0;
}

const dayMonth = new Intl.DateTimeFormat("en-GB", { day: "2-digit", month: "short" });
const fullDate = new Intl.DateTimeFormat("en-GB", {
  weekday: "long",
  day: "numeric",
  month: "long",
  year: "numeric",
});

/** The date gutter: `10 NOV`, in the reader's own time zone. */
export function gutterDate(instant: string): string {
  return dayMonth.format(new Date(instant)).toUpperCase();
}

/** A day in running text or on an axis: `22 Feb`, in the reader's own time zone. */
export function shortDate(instant: string): string {
  return dayMonth.format(new Date(instant)).replace(/^0/, "");
}

/** The same day, spelled out for a screen reader or a tooltip. */
export function longDate(instant: string): string {
  return fullDate.format(new Date(instant));
}

/** What each kind of account is called on screen. */
export const KIND_NAMES = {
  momo: "Mobile money",
  bank: "Bank",
  cash: "Cash",
  card: "Card",
} as const;

/** English plural rules, asked once. */
const PLURALS = new Intl.PluralRules("en-GB");

/**
 * The wording for `count` of something: `one` for 1, `other` otherwise — chosen by the
 * locale's own plural rules rather than by comparing with 1, so the sentence survives being
 * translated into a language with more than two forms.
 */
export function plural(count: number, forms: { one: string; other: string }): string {
  return PLURALS.select(count) === "one" ? forms.one : forms.other;
}
