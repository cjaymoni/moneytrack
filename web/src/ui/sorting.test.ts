import { describe, expect, it } from "vitest";
import type { Transaction } from "../api/types";
import { groupByPayee } from "./sorting";

function row(id: string, label: string, counterparty: string | null, extra: Partial<Transaction> = {}): Transaction {
  return {
    id,
    account_id: "a",
    direction: "out",
    amount: { minor_units: 100, currency: "GHS", display: "1.00" },
    booked_at: "2026-11-02T09:00:00Z",
    category_id: null,
    label,
    counterparty,
    note: null,
    status: "confirmed",
    source: "csv",
    recurring_id: null,
    ...extra,
  };
}

describe("groupByPayee", () => {
  it("groups by counterparty, ignoring case and spaces, biggest first", () => {
    const groups = groupByPayee([
      row("1", "CARD 1", "KFC Osu"),
      row("2", "CARD 2", " kfc osu "),
      row("3", "ECG PREPAID", null),
    ]);
    expect(groups.map((group) => [group.payee, group.field, group.rows.length])).toEqual([
      ["KFC Osu", "counterparty", 2],
      ["ECG PREPAID", "label", 1],
    ]);
  });

  it("falls back to the description when there is no counterparty", () => {
    const [uber] = groupByPayee([row("1", "UBER TRIP", null), row("2", "UBER TRIP", "  ")]);
    expect(uber?.field).toBe("label");
    expect(uber?.rows).toHaveLength(2);
  });

  it("keeps money in and money out from one name apart, and skips filed rows", () => {
    const groups = groupByPayee([
      row("1", "KOFI", "Kofi Mensah"),
      row("2", "KOFI", "Kofi Mensah", { direction: "in" }),
      row("3", "KOFI", "Kofi Mensah", { category_id: "c1" }),
    ]);
    expect(groups.map((group) => [group.direction, group.rows.length])).toEqual([
      ["in", 1],
      ["out", 1],
    ]);
  });

  it("is empty when everything has a category", () => {
    expect(groupByPayee([row("1", "X", null, { category_id: "c" })])).toEqual([]);
    expect(groupByPayee([])).toEqual([]);
  });
});
