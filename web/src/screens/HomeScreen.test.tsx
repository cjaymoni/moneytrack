import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import type { Account, Breakdown, Budget, InboxCount, Money, Outlook } from "../api/types";
import { renderWithClient } from "../test-utils";
import { HomeScreen } from "./HomeScreen";

const cedis = (minor_units: number, display: string): Money => ({ minor_units, currency: "GHS", display });

const MOMO: Account = {
  id: "a1",
  name: "MTN MoMo",
  kind: "momo",
  currency: "GHS",
  opening_balance: cedis(0, "0.00"),
  confirmed: cedis(399_700, "3997.00"),
  projected: cedis(399_700, "3997.00"),
};

const MONTH: Breakdown = {
  currency: "GHS",
  starts_on: "2026-11-01",
  ends_on: "2026-11-30",
  spending: {
    total: cedis(308_300, "3083.00"),
    lines: [
      line("c1", "Rent & housing", 250_000, "2500.00"),
      line("c2", "Utilities", 15_000, "150.00"),
      line("c3", "Transport", 7_100, "71.00"),
      line("c4", "Eating out", 8_000, "80.00"),
    ],
  },
  income: { total: cedis(630_000, "6300.00"), lines: [] },
  net: cedis(321_700, "3217.00"),
};

function line(id: string, name: string, minor: number, display: string) {
  const share = { basis_points: 0, display: "0%" };
  return { category_id: id, name, total: cedis(minor, display), share, own: cedis(minor, display), own_share: share, children: [] };
}

const SHORT: Outlook = {
  account: MOMO,
  from: "2026-11-23T00:00:00Z",
  until: "2026-12-23T00:00:00Z",
  start: MOMO.confirmed,
  start_level_bps: 9_000,
  points: [
    {
      transaction: {
        id: "t1",
        account_id: "a1",
        direction: "out",
        amount: cedis(450_000, "4500.00"),
        booked_at: "2026-12-02T12:00:00Z",
        category_id: null,
        label: "Rent",
        counterparty: null,
        note: null,
        status: "pending",
        source: "recurring",
        recurring_id: "r1",
      },
      balance_after: cedis(-50_300, "-503.00"),
      level_bps: 0,
    },
  ],
  low: cedis(-50_300, "-503.00"),
  low_transaction_id: "t1",
  high: MOMO.confirmed,
  zero_level_bps: 1_000,
  shortfall: "at",
  shortfall_transaction_id: "t1",
};

const NEAR_LIMIT: Budget = {
  id: "b1",
  category_id: "c4",
  name: "Eating out",
  period: "monthly",
  cap: cedis(10_000, "100.00"),
  warn_at_bps: 8_000,
  starts_on: "2026-11-01",
  ends_on: null,
  now: {
    period_starts_on: "2026-11-01",
    period_ends_on: "2026-11-30",
    spent: cedis(9_000, "90.00"),
    remaining: cedis(1_000, "10.00"),
    over_by: null,
    used_bps: 9_000,
    used_display: "90%",
    level: "warning",
  },
  expected: null,
};

/** What each endpoint answers, by path prefix. */
let answers: Record<string, unknown>;

beforeEach(() => {
  answers = {
    "/api/accounts": [MOMO],
    "/api/totals": [MOMO.confirmed],
    "/api/inbox/count": { total: 3, by_account: [{ account_id: "a1", waiting: 3 }] } satisfies InboxCount,
    "/api/outlook": [SHORT],
    "/api/breakdown": [MONTH],
    "/api/budgets": [NEAR_LIMIT],
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const path = url.split("?")[0] ?? url;
      if (!(path in answers)) return new Response("{}", { status: 404 });
      return new Response(JSON.stringify(answers[path]));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function renderHome() {
  return renderWithClient(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route path="/" element={<HomeScreen />} />
        <Route path="/accounts/new" element={<p>Add an account form</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("HomeScreen", () => {
  it("leads with what needs doing: transactions to review and an account going below zero", async () => {
    renderHome();

    expect(await screen.findByText(/3 imported transactions need a quick check/)).toBeTruthy();
    expect(screen.getByRole("link", { name: "Review now" }).getAttribute("href")).toBe("/inbox");
    expect((await screen.findByRole("alert")).textContent).toMatch(
      /MTN MoMo goes below zero on .*when Rent is paid, to −503\.00 GHS/,
    );
  });

  it("shows what you have, in total and per account", async () => {
    renderHome();

    const have = await screen.findByRole("region", { name: "You have" });
    expect(within(have).getAllByText("3,997.00").length).toBeGreaterThan(0);
    expect(within(have).getByRole("link", { name: /MTN MoMo/ }).getAttribute("href")).toBe("/accounts/a1");
  });

  it("sums up the month as the server worked it out, and names the three biggest spends", async () => {
    renderHome();

    const month = await screen.findByRole("region", { name: /so far$/ });
    expect(await within(month).findByText(/6,300\.00/)).toBeTruthy();
    expect(within(month).getByText(/3,083\.00/)).toBeTruthy();
    expect(within(month).getByText(/3,217\.00/)).toBeTruthy();
    const most = within(month).getByText(/Most went on/).textContent ?? "";
    expect(most).toMatch(/Rent & housing \(2,500\.00\), Utilities \(150\.00\) and Transport \(71\.00\)/);
    expect(most).not.toMatch(/Eating out/);
  });

  it("names only real categories, and says how much spending has none", async () => {
    answers["/api/breakdown"] = [
      {
        ...MONTH,
        spending: {
          ...MONTH.spending,
          lines: [{ ...line("x", "ignored", 300_000, "3000.00"), category_id: null, name: null }, ...MONTH.spending.lines.slice(0, 1)],
        },
      },
    ];
    renderHome();

    const month = await screen.findByRole("region", { name: /so far$/ });
    expect((await within(month).findByText(/Most went on/)).textContent).toBe("Most went on Rent & housing (2,500.00).");
    expect(within(month).getByText(/3,000\.00 GHS of spending has no category yet/)).toBeTruthy();
  });

  it("lists what is coming up, with the account it comes from", async () => {
    renderHome();

    const ahead = await screen.findByRole("list", { name: "Coming up" });
    expect(within(ahead).getByText("Rent")).toBeTruthy();
    expect(within(ahead).getByText("MTN MoMo")).toBeTruthy();
  });

  it("points at regular payments when nothing is planned", async () => {
    answers["/api/outlook"] = [{ ...SHORT, points: [], shortfall: "none", shortfall_transaction_id: null }];
    renderHome();

    expect(await screen.findByRole("link", { name: "Add a regular payment" })).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("names a budget close to its limit", async () => {
    renderHome();
    const budgets = await screen.findByRole("region", { name: "Budgets" });
    expect((await within(budgets).findByText(/Eating out/)).closest("li")?.textContent).toMatch(
      /90% used, 10\.00 GHS left/,
    );
  });

  it("says every budget is within its limit when none needs a look", async () => {
    answers["/api/budgets"] = [{ ...NEAR_LIMIT, now: NEAR_LIMIT.now && { ...NEAR_LIMIT.now, level: "under" } }];
    renderHome();
    expect(await screen.findByText("Your budget is within its limit.")).toBeTruthy();
  });

  it("stays quiet about budgets when there are none", async () => {
    answers["/api/budgets"] = [];
    answers["/api/inbox/count"] = { total: 0, by_account: [] };
    renderHome();
    await screen.findByRole("list", { name: "Coming up" });
    expect(screen.queryByRole("region", { name: "Budgets" })).toBeNull();
    expect(screen.queryByText(/need a quick check/)).toBeNull();
  });

  it("sends someone with no accounts to add their first", async () => {
    answers["/api/accounts"] = [];
    renderHome();
    expect(await screen.findByText("Add an account form")).toBeTruthy();
  });
});
