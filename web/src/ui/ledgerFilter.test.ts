import { describe, expect, it } from "vitest";
import type { LedgerEntry } from "../api/types";
import { NO_FILTER, isFiltering, matches } from "./ledgerFilter";

function entry(label: string, extra: Partial<LedgerEntry["transaction"]> = {}): LedgerEntry {
  const money = { minor_units: 100, currency: "GHS", display: "1.00" };
  return {
    transaction: {
      id: label,
      account_id: "a",
      direction: "out",
      amount: money,
      booked_at: "2026-11-02T09:00:00Z",
      category_id: null,
      label,
      counterparty: null,
      note: null,
      status: "confirmed",
      source: "manual",
      recurring_id: null,
      ...extra,
    },
    balance_after: money,
  };
}

describe("matches", () => {
  it("finds every typed word across description, payee, note and category, in any case", () => {
    const kfc = entry("CARD 0412", { counterparty: "KFC Osu", note: "lunch with Esi" });
    expect(matches(kfc, { text: "kfc", show: "all" })).toBe(true);
    expect(matches(kfc, { text: "osu LUNCH", show: "all" })).toBe(true);
    expect(matches(kfc, { text: "kfc dinner", show: "all" })).toBe(false);
    expect(matches(kfc, { text: "eating", show: "all" }, "Eating out")).toBe(true);
  });

  it("keeps only uncategorised or only planned rows when asked", () => {
    const filed = entry("Rent", { category_id: "c" });
    const planned = entry("Salary", { status: "pending" });
    expect(matches(filed, { text: "", show: "uncategorised" })).toBe(false);
    expect(matches(planned, { text: "", show: "uncategorised" })).toBe(true);
    expect(matches(planned, { text: "", show: "planned" })).toBe(true);
    expect(matches(filed, { text: "", show: "planned" })).toBe(false);
  });
});

describe("isFiltering", () => {
  it("is off with no words and everything shown, spaces included", () => {
    expect(isFiltering(NO_FILTER)).toBe(false);
    expect(isFiltering({ text: "   ", show: "all" })).toBe(false);
    expect(isFiltering({ text: "x", show: "all" })).toBe(true);
    expect(isFiltering({ text: "", show: "planned" })).toBe(true);
  });
});
