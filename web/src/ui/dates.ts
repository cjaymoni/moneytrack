// Turning the date a person picked into the instant a row is booked at, and back.
//
// Dates, not money: which day a row falls on is read on the reader's own clock, and the
// server's zone-aware rules read the instant this produces the same way.

const pad = (value: number) => String(value).padStart(2, "0");

/** A date as `<input type="date">` writes it, in the reader's own zone. */
export function localDate(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** The local date an RFC 3339 instant falls on, as `<input type="date">` writes it. */
export function localDateOf(instant: string): string {
  return localDate(new Date(instant));
}

/**
 * The instant to book a row at, from the day the person picked.
 *
 * Midday on that day in the reader's zone: far enough from midnight that no zone moves it
 * onto a neighbouring date. `undefined` for a value that is not a date.
 */
export function middayOn(picked: string): string | undefined {
  const [year, month, day] = picked.split("-").map(Number);
  if (!year || !month || !day) return undefined;
  return new Date(year, month - 1, day, 12, 0, 0).toISOString();
}

/**
 * The instant to keep a row at after an edit.
 *
 * If the day was not changed, the original instant stands — a row recorded at 07:40 stays at
 * 07:40, and two rows on one day keep their order. A new day is midday on it.
 */
export function keptOrMoved(original: string, picked: string): string {
  if (picked === localDateOf(original)) return original;
  return middayOn(picked) ?? original;
}
