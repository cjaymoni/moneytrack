import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import type { Outlook, OutlookPoint } from "../api/types";
import { renderWithClient } from "../test-utils";
import { OutlookScreen } from "./OutlookScreen";

const cedis = (minor_units: number, display: string) => ({ minor_units, currency: "GHS", display });

const ACCOUNT = {
  id: "a1",
  name: "MTN MoMo",
  kind: "momo" as const,
  currency: "GHS",
  opening_balance: cedis(0, "0.00"),
  confirmed: cedis(50_000, "500.00"),
  projected: cedis(80_000, "800.00"),
};

function point(id: string, label: string, at: string, direction: "in" | "out", amount: string, after: [number, string], level: number): OutlookPoint {
  return {
    transaction: {
      id,
      account_id: "a1",
      direction,
      amount: cedis(0, amount),
      booked_at: at,
      category_id: null,
      label,
      counterparty: null,
      note: null,
      status: "pending",
      source: "manual",
    },
    balance_after: cedis(after[0], after[1]),
    level_bps: level,
  };
}

const SHORT: Outlook = {
  account: ACCOUNT,
  from: "2026-11-22T00:00:00Z",
  until: "2027-02-20T00:00:00Z",
  start: cedis(50_000, "500.00"),
  start_level_bps: 7_000,
  points: [
    point("t1", "School fees", "2026-11-27T12:00:00Z", "out", "300.00", [20_000, "200.00"], 4_000),
    point("t2", "Rent", "2026-12-02T12:00:00Z", "out", "400.00", [-20_000, "-200.00"], 0),
    point("t3", "Salary", "2026-12-12T12:00:00Z", "in", "1000.00", [80_000, "800.00"], 10_000),
  ],
  low: cedis(-20_000, "-200.00"),
  low_transaction_id: "t2",
  high: cedis(80_000, "800.00"),
  zero_level_bps: 2_000,
  shortfall: "at",
  shortfall_transaction_id: "t2",
};

let answer: Outlook[];
let asked: string[];

beforeEach(() => {
  asked = [];
  answer = [SHORT];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      asked.push(url);
      return new Response(JSON.stringify(answer));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function renderAt(path: string) {
  return renderWithClient(
    <MemoryRouter initialEntries={[path]}>
      <OutlookScreen />
    </MemoryRouter>,
  );
}

describe("OutlookScreen", () => {
  it("names the row that runs the account short, in words", async () => {
    renderAt("/outlook");

    const verdict = await screen.findByRole("alert");
    expect(verdict.textContent).toMatch(/Runs short on .*Rent takes it to −200\.00 GHS/);
    expect(screen.getByText("Runs short here")).toBeTruthy();
  });

  it("prints the low and every balance as the server sent them", async () => {
    renderAt("/outlook");

    const section = await screen.findByRole("region", { name: /MTN MoMo/ });
    expect(within(section).getAllByText("−200.00").length).toBeGreaterThan(0);
    expect(within(section).getByText("800.00")).toBeTruthy();
    expect(within(section).getByText("+1,000.00")).toBeTruthy();
  });

  it("draws the line from the server's levels, and marks the low", async () => {
    const { container } = renderAt("/outlook");
    await screen.findByRole("alert");

    // Start at level 7 000, then each row's level: y = 190 − level × 180 / 10 000.
    const path = container.querySelector(".line__path")?.getAttribute("d") ?? "";
    const heights = [...path.matchAll(/V ([\d.]+)/g)].map((match) => Number(match[1]));
    expect(path.startsWith("M 0 64")).toBe(true);
    expect(heights).toEqual([118, 190, 10, 10]);
    expect(container.querySelector(".line__low")).toBeTruthy();
    expect(container.querySelector(".line__zero")?.getAttribute("y1")).toBe("154");
  });

  it("says when an account stays above zero", async () => {
    answer = [{ ...SHORT, shortfall: "none", shortfall_transaction_id: null, points: [], low_transaction_id: null, zero_level_bps: 0 }];
    renderAt("/outlook?days=30");

    expect(await screen.findByText("Stays above zero for the next 30 days.")).toBeTruthy();
    expect(asked[0]).toBe("/api/outlook?days=30");
  });

  it("says when an account is short already", async () => {
    answer = [{ ...SHORT, shortfall: "already", shortfall_transaction_id: null, start: cedis(-2_000, "-20.00") }];
    renderAt("/outlook");

    expect((await screen.findByRole("alert")).textContent).toMatch(/Already short: it holds −20\.00 GHS now/);
  });
});
