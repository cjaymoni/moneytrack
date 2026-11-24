import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Category, Inbox, InboxItem, Transaction } from "../api/types";
import { resetCsrf } from "../api/client";
import { renderInApp } from "../test-utils";
import { InboxScreen, reasons } from "./InboxScreen";

const cedis = (minor: number, display: string) => ({ minor_units: minor, currency: "GHS", display });

const GROCERIES: Category = { id: "c-food", name: "Food & groceries", parent_id: null, kind: "expense", sort_order: 0 };
const EATING_OUT: Category = { id: "c-out", name: "Eating out", parent_id: null, kind: "expense", sort_order: 1 };

const TYPED: Transaction = {
  id: "t-typed",
  account_id: "a1",
  direction: "out",
  amount: cedis(12_050, "120.50"),
  booked_at: "2026-10-03T14:00:00Z",
  category_id: null,
  label: "Shopping",
  counterparty: "Shoprite",
  note: null,
  status: "confirmed",
  source: "manual",
  recurring_id: null,
};

function line(id: string, label: string, extra: Partial<InboxItem> = {}): InboxItem {
  return {
    id,
    source: "csv",
    file_name: "oct.csv",
    line_no: 2,
    account_id: "a1",
    account_name: "GCB Bank",
    direction: "out",
    amount: cedis(12_050, "120.50"),
    booked_at: "2026-10-03T00:00:00Z",
    label,
    counterparty: "SHOPRITE",
    category_id: "c-food",
    category_confidence_bps: 8_000,
    reported_balance: null,
    suspects: [],
    twins: [],
    ...extra,
  };
}

let inbox: Inbox;
let calls: { url: string; method: string; body: unknown }[];

beforeEach(() => {
  calls = [];
  resetCsrf();
  inbox = { total: 0, items: [] };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      calls.push({ url, method, body: init?.body ? JSON.parse(String(init.body)) : null });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (url === "/api/inbox") return new Response(JSON.stringify(inbox));
      if (url === "/api/categories") return new Response(JSON.stringify([GROCERIES, EATING_OUT]));
      return new Response(JSON.stringify({}));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

const posted = (url: string) => calls.find((call) => call.method === "POST" && call.url === url)?.body;

describe("reasons", () => {
  it("joins the signals the way a person would say them", () => {
    expect(reasons(["same_amount"])).toBe("the same amount");
    expect(reasons(["same_amount", "close_in_time", "same_counterparty"])).toBe(
      "the same amount, within a day and the same payee",
    );
    expect(reasons(["something_new"])).toBe("");
  });
});

describe("InboxScreen", () => {
  it("says so when nothing is waiting, and points at importing", async () => {
    renderInApp(<InboxScreen />);

    expect(await screen.findByText(/Nothing to review/)).toBeTruthy();
    expect(screen.getByRole("link", { name: "Import a statement" })).toBeTruthy();
  });

  it("shows each line with where it came from, filed where the server proposed", async () => {
    inbox = { total: 1, items: [line("i1", "POS PURCHASE")] };
    renderInApp(<InboxScreen />);

    expect(await screen.findByText("POS PURCHASE")).toBeTruthy();
    expect(screen.getByText("SHOPRITE · GCB Bank · oct.csv, line 2")).toBeTruthy();
    expect(screen.getByText("−120.50")).toBeTruthy();
    const proposed = await screen.findByRole("button", { name: "Food & groceries" });
    expect(proposed.getAttribute("aria-pressed")).toBe("true");
  });

  it("accepts a line under the category the person chose instead", async () => {
    const user = userEvent.setup();
    inbox = { total: 1, items: [line("i1", "POS PURCHASE")] };
    renderInApp(<InboxScreen />);

    await user.click(await screen.findByRole("button", { name: "Eating out" }));
    await user.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() =>
      expect(posted("/api/inbox/accept")).toEqual({ items: [{ id: "i1", category_id: "c-out" }] }),
    );
  });

  it("offers a likely duplicate to merge into, and says why it looks like one", async () => {
    const user = userEvent.setup();
    inbox = {
      total: 1,
      items: [
        line("i1", "POS PURCHASE", {
          suspects: [
            { transaction: TYPED, score_bps: 9_000, certain: false, signals: ["same_amount", "close_in_time"] },
          ],
        }),
      ],
    };
    renderInApp(<InboxScreen />);

    const card = (await screen.findByText("POS PURCHASE")).closest("li") as HTMLElement;
    expect(within(card).getByText(/Might already be in your accounts/)).toBeTruthy();
    expect(within(card).getByText(/the same amount and within a day/)).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Add as new" })).toBeTruthy();
    await user.click(within(card).getByRole("button", { name: "Same payment" }));

    await waitFor(() => expect(posted("/api/inbox/i1/merge")).toEqual({ transaction_id: "t-typed" }));
  });

  it("sets a line aside without accepting it", async () => {
    const user = userEvent.setup();
    inbox = { total: 1, items: [line("i1", "POS PURCHASE")] };
    renderInApp(<InboxScreen />);

    await user.click(await screen.findByRole("button", { name: "Ignore" }));

    await waitFor(() => expect(posted("/api/inbox/i1/reject")).toEqual({}));
  });

  it("accepts every line with no likely duplicate at once, leaving the doubtful one", async () => {
    const user = userEvent.setup();
    inbox = {
      total: 3,
      items: [
        line("i1", "FARE", { category_id: null }),
        line("i2", "SALARY", { direction: "in", category_id: null }),
        line("i3", "POS PURCHASE", {
          suspects: [{ transaction: TYPED, score_bps: 9_000, certain: false, signals: ["same_amount"] }],
        }),
      ],
    };
    renderInApp(<InboxScreen />);

    await user.click(await screen.findByRole("button", { name: "Add all 2 that aren't possible duplicates" }));

    await waitFor(() =>
      expect(posted("/api/inbox/accept")).toEqual({
        items: [
          { id: "i1", category_id: null },
          { id: "i2", category_id: null },
        ],
      }),
    );
  });

  it("leaves a line that matches one from another upload out of the bulk accept, and says why", async () => {
    inbox = {
      total: 3,
      items: [
        line("i1", "FARE"),
        line("i2", "KENKEY"),
        line("i3", "POS SHOPRITE", {
          twins: [{ id: "i9", file_name: "oct.ofx", line_no: 4, label: "SHOPRITE ACCRA" }],
        }),
      ],
    };
    renderInApp(<InboxScreen />);

    expect(await screen.findByRole("button", { name: "Add all 2 that aren't possible duplicates" })).toBeTruthy();
    expect(screen.getByText("Also in another imported file")).toBeTruthy();
    expect(screen.getByText(/oct.ofx line 4/)).toBeTruthy();
  });

  it("says when more are waiting than are shown", async () => {
    inbox = { total: 450, items: [line("i1", "POS PURCHASE")] };
    renderInApp(<InboxScreen />);

    expect(await screen.findByText("Showing the oldest 1 of 450. More appear as you clear these.")).toBeTruthy();
  });
});
