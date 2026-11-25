import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import { renderInApp as render } from "../test-utils";
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
      recurring_id: null,
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
    expect(screen.getByText("planned")).toBeTruthy();
  });

  it("says what an empty account opened with", () => {
    render(<LedgerList now={NOW} opening={cedis(5_000, "5000")} currency="XOF" entries={[]} />);

    expect(screen.getByText("No transactions yet.")).toBeTruthy();
    expect(screen.getByText(/Starting balance 5,000 XOF/)).toBeTruthy();
  });

  it("shows each row's category, and says so when there is none", () => {
    const filed = entry("1", "Kenkey", "2026-11-02T09:00:00Z", "out", cedis(500, "5.00"), cedis(-500, "-5.00"));
    filed.transaction.category_id = "c-child";
    render(
      <LedgerList
        now={NOW}
        opening={cedis(0, "0.00")}
        currency="GHS"
        categories={[
          { id: "c-root", name: "Food & groceries", parent_id: null, kind: "expense", sort_order: 0 },
          { id: "c-child", name: "Chop bar", parent_id: "c-root", kind: "expense", sort_order: 1 },
        ]}
        entries={[
          filed,
          entry("2", "Trotro", "2026-11-03T09:00:00Z", "out", cedis(500, "5.00"), cedis(-1_000, "-10.00")),
        ]}
      />,
    );

    expect(screen.getByRole("button", { name: /Food & groceries · Chop bar/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: "No category. Add category" })).toBeTruthy();
  });

  it("marks a correction to a count as counted", () => {
    const fixed = entry("1", "Balance updated to 340.00 GHS", "2026-11-02T09:00:00Z", "out", cedis(4_000, "40.00"), cedis(34_000, "340.00"));
    fixed.transaction.source = "adjustment";
    render(<LedgerList now={NOW} opening={cedis(38_000, "380.00")} currency="GHS" entries={[fixed]} />);

    expect(screen.getByText("Adjustment")).toBeTruthy();
    expect(screen.queryByText("By hand")).toBeNull();
  });

  it("stamps a row read off a statement and checked in the inbox", () => {
    const imported = entry("1", "POS SHOPRITE", "2026-11-02T00:00:00Z", "out", cedis(2_000, "20.00"), cedis(36_000, "360.00"));
    imported.transaction.source = "csv";
    render(<LedgerList now={NOW} opening={cedis(38_000, "380.00")} currency="GHS" entries={[imported]} />);

    expect(screen.getByText("Imported")).toBeTruthy();
    expect(screen.queryByText("By hand")).toBeNull();
  });

  it("offers to confirm an expected row that is due, sending the row back whole", async () => {
    const calls: { url: string; init: RequestInit | undefined }[] = [];
    const fetchMock = async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      return new Response(JSON.stringify({ ...due.transaction, status: "confirmed" }));
    };
    const original = globalThis.fetch;
    globalThis.fetch = fetchMock as typeof fetch;
    const due = entry("1", "Rent", "2026-11-01T00:00:00Z", "out", cedis(140_000, "1400.00"), cedis(-140_000, "-1400.00"), "pending");
    due.transaction.recurring_id = "r-rent";
    due.transaction.source = "recurring";
    const later = entry("2", "Rent", "2026-12-01T00:00:00Z", "out", cedis(140_000, "1400.00"), cedis(-280_000, "-2800.00"), "pending");
    later.transaction.recurring_id = "r-rent";
    try {
      render(<LedgerList now={NOW} opening={cedis(0, "0.00")} currency="GHS" entries={[due, later]} />);

      // Only the one that is due gets the button; both say they repeat.
      expect(screen.getAllByRole("button", { name: "Mark as paid" })).toHaveLength(1);
      expect(screen.getAllByText("repeats")).toHaveLength(2);

      screen.getByRole("button", { name: "Mark as paid" }).click();
      await new Promise((resolve) => setTimeout(resolve, 50));
      const sent = calls.find((call) => call.init?.method === "PATCH");
      expect(sent?.url).toBe("/api/transactions/1");
      expect(JSON.parse(String(sent?.init?.body))).toEqual({
        direction: "out",
        amount: "1400.00",
        booked_at: "2026-11-01T00:00:00Z",
        label: "Rent",
        counterparty: null,
        note: null,
        status: "confirmed",
        category_id: null,
      });
    } finally {
      globalThis.fetch = original;
    }
  });

  it("lists the weeks ahead and counts the rest of a year a series has written", () => {
    const soon = entry("1", "Market", "2026-11-29T00:00:00Z", "out", cedis(2_500, "25.00"), cedis(0, "0.00"), "pending");
    const far = [1, 2, 3].map((month) =>
      entry(`f${month}`, "Rent", `2027-0${month + 1}-02T00:00:00Z`, "out", cedis(1, "1.00"), cedis(0, "0.00"), "pending"),
    );
    render(<LedgerList now={NOW} opening={cedis(0, "0.00")} currency="GHS" entries={[soon, ...far]} />);

    expect(screen.getByText("Market")).toBeTruthy();
    expect(screen.queryByText("Rent")).toBeNull();
    expect(screen.getByText(/3 more planned transactions after the next 45 days/)).toBeTruthy();
    expect(screen.getByRole("link", { name: "See them in the forecast" }).getAttribute("href")).toBe("/outlook?days=180");
  });

  it("opens on the most recent transactions, with the rest a click away", async () => {
    const many = Array.from({ length: 25 }, (_, index) =>
      entry(String(index), `Row ${index}`, `2026-10-${String(index + 1).padStart(2, "0")}T09:00:00Z`, "out", cedis(100, "1.00"), cedis(0, "0.00")),
    );
    render(<LedgerList now={NOW} opening={cedis(0, "0.00")} currency="GHS" entries={many} />);
    expect(screen.queryByText("Row 0")).toBeNull();
    expect(screen.getByText("Row 24")).toBeTruthy();

    screen.getByRole("button", { name: "Show 5 earlier transactions" }).click();

    expect(await screen.findByText("Row 0")).toBeTruthy();
    expect(screen.getByText("Starting balance")).toBeTruthy();
  });

  it("shows only what matches a search, or says nothing does", () => {
    const rows = [
      entry("1", "Waakye", "2026-11-02T09:00:00Z", "out", cedis(100, "1.00"), cedis(0, "0.00")),
      entry("2", "Trotro", "2026-11-03T09:00:00Z", "out", cedis(100, "1.00"), cedis(0, "0.00")),
    ];
    const { rerender } = render(
      <LedgerList now={NOW} opening={cedis(0, "0.00")} currency="GHS" entries={rows} filter={{ text: "waak", show: "all" }} />,
    );
    expect(screen.getByText("Waakye")).toBeTruthy();
    expect(screen.queryByText("Trotro")).toBeNull();

    rerender(
      <LedgerList now={NOW} opening={cedis(0, "0.00")} currency="GHS" entries={rows} filter={{ text: "kenkey", show: "all" }} />,
    );
    expect(screen.getByText(/No transactions match/)).toBeTruthy();
  });

  it("says a row can be edited without needing a hover", () => {
    render(
      <LedgerList
        now={NOW}
        opening={cedis(0, "0.00")}
        currency="GHS"
        entries={[entry("1", "Waakye", "2026-11-02T09:00:00Z", "out", cedis(100, "1.00"), cedis(0, "0.00"))]}
      />,
    );
    expect(screen.getByRole("button", { name: "Waakye, edit" })).toBeTruthy();
  });
});
