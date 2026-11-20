import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import type { LedgerEntry, Money } from "../api/types";
import { LedgerList } from "./LedgerList";

const cedis = (minor_units: number, display: string): Money => ({
  minor_units,
  currency: "GHS",
  display,
});

function entry(
  id: string,
  label: string,
  booked_at: string,
  direction: "in" | "out",
  amount: Money,
  after: Money,
  status: "confirmed" | "pending" = "confirmed",
): LedgerEntry {
  return {
    transaction: {
      id,
      account_id: "a",
      direction,
      amount,
      booked_at,
      category_id: null,
      label,
      counterparty: null,
      note: null,
      status,
      source: "manual",
    },
    balance_after: after,
  };
}

const NOW = new Date("2026-11-20T12:00:00Z");

describe("LedgerList", () => {
  it("runs history through a TODAY rule into the rows still expected", () => {
    render(
      <LedgerList
        now={NOW}
        opening={cedis(10_000, "100.00")}
        currency="GHS"
        entries={[
          entry("1", "Salary", "2026-11-01T09:00:00Z", "in", cedis(200_000, "2000.00"), cedis(210_000, "2100.00")),
          entry("2", "Rent", "2026-11-25T09:00:00Z", "out", cedis(300_000, "3000.00"), cedis(-90_000, "-900.00"), "pending"),
        ]}
      />,
    );

    const lists = screen.getAllByRole("list");
    expect(lists).toHaveLength(2);
    const [history, forecast] = lists as [HTMLElement, HTMLElement];
    expect(within(history).getByText("Salary")).toBeTruthy();
    expect(within(forecast).getByText("Rent")).toBeTruthy();
    expect(screen.getByRole("separator", { name: "Today" })).toBeTruthy();
  });

  it("prints the running balance the server walked, and does not work out its own", () => {
    // The server's figure is deliberately not the sum of the rows above it. If this client
    // ever started adding up, this test would see the sum instead.
    render(
      <LedgerList
        now={NOW}
        opening={cedis(0, "0.00")}
        currency="GHS"
        entries={[
          entry("1", "Waakye", "2026-11-02T09:00:00Z", "out", cedis(1_500, "15.00"), cedis(123_456, "1234.56")),
        ]}
      />,
    );

    expect(screen.getByText("−15.00")).toBeTruthy();
    expect(screen.getByText("1,234.56")).toBeTruthy();
  });

  it("marks a row a person typed, and leaves an expected one unmarked", () => {
    render(
      <LedgerList
        now={NOW}
        opening={cedis(0, "0.00")}
        currency="GHS"
        entries={[
          entry("1", "Trotro", "2026-11-02T09:00:00Z", "out", cedis(500, "5.00"), cedis(-500, "-5.00")),
          entry("2", "School fees", "2026-12-02T09:00:00Z", "out", cedis(500, "5.00"), cedis(-1_000, "-10.00"), "pending"),
        ]}
      />,
    );

    expect(screen.getAllByText("By hand")).toHaveLength(1);
    expect(screen.getByText("expected")).toBeTruthy();
  });

  it("says what an empty account opened with", () => {
    render(<LedgerList now={NOW} opening={cedis(5_000, "5000")} currency="XOF" entries={[]} />);

    expect(screen.getByText("Nothing recorded yet.")).toBeTruthy();
    expect(screen.getByText(/Opened with 5,000 XOF/)).toBeTruthy();
  });
});
