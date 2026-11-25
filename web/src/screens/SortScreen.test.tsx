import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transaction } from "../api/types";
import { resetCsrf } from "../api/client";
import { renderInApp } from "../test-utils";
import { SortScreen } from "./SortScreen";

function row(id: string, label: string, counterparty: string | null, category_id: string | null = null): Transaction {
  return {
    id,
    account_id: "a1",
    direction: "out",
    amount: { minor_units: 3_550, currency: "GHS", display: "35.50" },
    booked_at: "2026-11-07T09:00:00Z",
    category_id,
    label,
    counterparty,
    note: null,
    status: "confirmed",
    source: "csv",
    recurring_id: null,
  };
}

const CATEGORIES = [
  { id: "c-transport", name: "Transport", parent_id: null, kind: "expense", sort_order: 0 },
  { id: "c-salary", name: "Salary", parent_id: null, kind: "income", sort_order: 1 },
];

let rows: Transaction[];
let posted: { url: string; body: unknown }[];

beforeEach(() => {
  resetCsrf();
  rows = [row("t1", "UBER TRIP", null), row("t2", "UBER TRIP", null), row("t3", "KFC OSU", "KFC OSU"), row("t4", "Rent", null, "c-rent")];
  posted = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (url === "/api/transactions" && (init?.method ?? "GET") === "GET") return new Response(JSON.stringify(rows));
      if (url === "/api/categories") return new Response(JSON.stringify(CATEGORIES));
      if (url === "/api/accounts") return new Response(JSON.stringify([{ id: "a1", name: "MTN MoMo" }]));
      if (url === "/api/transactions/category") {
        const body = JSON.parse(String(init?.body));
        posted.push({ url, body });
        return new Response(JSON.stringify({ filed: body.transaction_ids.length }));
      }
      return new Response("{}", { status: 404 });
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

describe("SortScreen", () => {
  it("groups uncategorised transactions by payee, biggest group first", async () => {
    renderInApp(<SortScreen />);
    const groups = within(await screen.findByRole("list", { name: "Payees to sort" }));
    const names = groups.getAllByRole("heading").map((heading) => heading.textContent);
    expect(names).toEqual(["UBER TRIP", "KFC OSU"]);
    expect(screen.getByText(/2 transactions · money out/)).toBeTruthy();
    expect(screen.queryByText("Rent")).toBeNull();
  });

  it("files a whole group with one click and remembers the payee", async () => {
    const user = userEvent.setup();
    renderInApp(<SortScreen />);
    const uber = within(await screen.findByRole("group", { name: "Category for UBER TRIP" }));

    await user.click(uber.getByRole("button", { name: "Transport" }));

    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]?.body).toEqual({
      transaction_ids: ["t1", "t2"],
      category_id: "c-transport",
      remember: { matched_field: "label", pattern: "UBER TRIP" },
    });
    expect(await screen.findByText(/Filed 2 from/)).toBeTruthy();
  });

  it("files without remembering when the box is cleared, and can undo", async () => {
    const user = userEvent.setup();
    renderInApp(<SortScreen />);
    await screen.findByRole("list", { name: "Payees to sort" });
    await user.click(screen.getByRole("checkbox", { name: /File future transactions from KFC OSU/ }));
    const kfc = within(screen.getByRole("group", { name: "Category for KFC OSU" }));

    await user.click(kfc.getByRole("button", { name: "Transport" }));
    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]?.body).toEqual({ transaction_ids: ["t3"], category_id: "c-transport" });

    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(posted).toHaveLength(2));
    expect(posted[1]?.body).toEqual({ transaction_ids: ["t3"], category_id: null });
  });

  it("says so when everything has a category", async () => {
    rows = [row("t4", "Rent", null, "c-rent")];
    renderInApp(<SortScreen />);
    expect(await screen.findByText(/Everything has a category/)).toBeTruthy();
  });
});
