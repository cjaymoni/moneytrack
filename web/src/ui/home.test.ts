import { describe, expect, it } from "vitest";
import type { Account, Budget, Outlook, OutlookPoint, Standing } from "../api/types";
import { budgetsToWatch, shortfalls, upcoming } from "./home";

const cedis = (minor_units: number, display: string) => ({ minor_units, currency: "GHS", display });

function account(id: string, name: string): Account {
  return {
    id,
    name,
    kind: "momo",
    currency: "GHS",
    opening_balance: cedis(0, "0.00"),
    confirmed: cedis(50_000, "500.00"),
    projected: cedis(50_000, "500.00"),
  };
}

function point(id: string, label: string, at: string, after: [number, string]): OutlookPoint {
  return {
    transaction: {
      id,
      account_id: "a",
      direction: "out",
      amount: cedis(10_000, "100.00"),
      booked_at: at,
      category_id: null,
      label,
      counterparty: null,
      note: null,
      status: "pending",
      source: "recurring",
      recurring_id: "r",
    },
    balance_after: cedis(after[0], after[1]),
    level_bps: 0,
  };
}

function outlook(owner: Account, points: OutlookPoint[], extra: Partial<Outlook> = {}): Outlook {
  return {
    account: owner,
    from: "2026-11-23T00:00:00Z",
    until: "2026-12-23T00:00:00Z",
    start: owner.confirmed,
    start_level_bps: 5_000,
    points,
    low: owner.confirmed,
    low_transaction_id: null,
    high: owner.confirmed,
    zero_level_bps: 0,
    shortfall: "none",
    shortfall_transaction_id: null,
    ...extra,
  };
}

describe("upcoming", () => {
  const momo = account("a", "MoMo");
  const bank = account("b", "Bank");

  it("merges every account's planned payments, soonest first, with the account named", () => {
    const merged = upcoming(
      [
        outlook(momo, [point("1", "Rent", "2026-12-02T12:00:00Z", [0, "0"]), point("2", "Water", "2026-12-20T12:00:00Z", [0, "0"])]),
        outlook(bank, [point("3", "School fees", "2026-11-28T12:00:00Z", [0, "0"])]),
      ],
      10,
    );
    expect(merged.map((each) => [each.transaction.label, each.accountName])).toEqual([
      ["School fees", "Bank"],
      ["Rent", "MoMo"],
      ["Water", "MoMo"],
    ]);
  });

  it("keeps only as many as asked for", () => {
    const many = outlook(momo, [
      point("1", "A", "2026-12-01T12:00:00Z", [0, "0"]),
      point("2", "B", "2026-12-02T12:00:00Z", [0, "0"]),
      point("3", "C", "2026-12-03T12:00:00Z", [0, "0"]),
    ]);
    expect(upcoming([many], 2)).toHaveLength(2);
    expect(upcoming([many], 0)).toEqual([]);
    expect(upcoming([many], -1)).toEqual([]);
  });

  it("orders payments on the same day by name, so a reload never reshuffles them", () => {
    const sameDay = outlook(momo, [
      point("1", "Water", "2026-12-01T12:00:00Z", [0, "0"]),
      point("2", "Light", "2026-12-01T12:00:00Z", [0, "0"]),
    ]);
    expect(upcoming([sameDay], 5).map((each) => each.transaction.label)).toEqual(["Light", "Water"]);
  });

  it("is empty when nothing is planned", () => {
    expect(upcoming([], 5)).toEqual([]);
    expect(upcoming([outlook(momo, [])], 5)).toEqual([]);
  });
});

describe("shortfalls", () => {
  const momo = account("a", "MoMo");

  it("names the payment that takes an account below zero, and the balance after it", () => {
    const rent = point("rent", "Rent", "2026-12-02T12:00:00Z", [-20_000, "-200.00"]);
    const [found, ...rest] = shortfalls([
      outlook(momo, [rent], { shortfall: "at", shortfall_transaction_id: "rent", low: cedis(-20_000, "-200.00") }),
    ]);
    expect(rest).toEqual([]);
    expect(found).toMatchObject({
      accountName: "MoMo",
      already: false,
      on: "2026-12-02T12:00:00Z",
      cause: "Rent",
      low: { display: "-200.00" },
    });
  });

  it("says an account is below zero already, with today's balance", () => {
    const [found] = shortfalls([outlook(momo, [], { shortfall: "already", start: cedis(-5_000, "-50.00") })]);
    expect(found).toMatchObject({ already: true, on: null, cause: null, low: { display: "-50.00" } });
  });

  it("still reports a shortfall whose payment it cannot find, using the forecast's low", () => {
    const [found] = shortfalls([
      outlook(momo, [], { shortfall: "at", shortfall_transaction_id: "gone", low: cedis(-100, "-1.00") }),
    ]);
    expect(found).toMatchObject({ already: false, on: null, cause: null, low: { display: "-1.00" } });
  });

  it("is empty when every account stays above zero", () => {
    expect(shortfalls([outlook(momo, [])])).toEqual([]);
  });
});

describe("budgetsToWatch", () => {
  function standing(level: Standing["level"], used_bps: number): Standing {
    return {
      period_starts_on: "2026-11-01",
      period_ends_on: "2026-11-30",
      spent: cedis(0, "0.00"),
      remaining: cedis(0, "0.00"),
      over_by: null,
      used_bps,
      used_display: `${used_bps / 100}%`,
      level,
    };
  }
  function budget(id: string, now: Standing | null): Budget {
    return {
      id,
      category_id: null,
      name: id,
      period: "monthly",
      cap: cedis(10_000, "100.00"),
      warn_at_bps: 8_000,
      starts_on: "2026-11-01",
      ends_on: null,
      now,
      expected: null,
    };
  }

  it("keeps the ones near or over their limit, worst first", () => {
    const picked = budgetsToWatch([
      budget("fine", standing("under", 2_000)),
      budget("near", standing("warning", 8_500)),
      budget("nearer", standing("warning", 9_500)),
      budget("over", standing("over", 11_000)),
    ]);
    expect(picked.map((each) => each.id)).toEqual(["over", "nearer", "near"]);
  });

  it("leaves out ended budgets, which have no standing", () => {
    expect(budgetsToWatch([budget("ended", null)])).toEqual([]);
  });
});
