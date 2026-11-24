import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Budget, Standing } from "../api/types";
import { resetCsrf } from "../api/client";
import { renderWithClient } from "../test-utils";
import { BudgetsScreen, Tiers } from "./BudgetsScreen";

const cedis = (minor_units: number, display: string) => ({ minor_units, currency: "GHS", display });

function standing(spent: [number, string], remaining: [number, string], used_bps: number, level: Standing["level"], over_by: [number, string] | null = null): Standing {
  return {
    period_starts_on: "2026-11-01",
    period_ends_on: "2026-11-30",
    spent: cedis(...spent),
    remaining: cedis(...remaining),
    over_by: over_by ? cedis(...over_by) : null,
    used_bps,
    used_display: `${Math.floor(used_bps / 100)}%`,
    level,
  };
}

const FOOD: Budget = {
  id: "b-food",
  category_id: "c-food",
  name: "Food & groceries",
  period: "monthly",
  cap: cedis(20_000, "200.00"),
  warn_at_bps: 8_000,
  starts_on: "2026-11-01",
  ends_on: null,
  now: standing([24_600, "246.00"], [-4_600, "-46.00"], 12_300, "over", [4_600, "46.00"]),
  expected: standing([24_600, "246.00"], [-4_600, "-46.00"], 12_300, "over", [4_600, "46.00"]),
};

const TRANSPORT: Budget = {
  ...FOOD,
  id: "b-transport",
  category_id: "c-transport",
  name: "Transport",
  cap: cedis(10_000, "100.00"),
  now: standing([3_000, "30.00"], [7_000, "70.00"], 3_000, "under"),
  expected: standing([9_000, "90.00"], [1_000, "10.00"], 9_000, "warning"),
};

const ENDED: Budget = { ...FOOD, id: "b-old", cap: cedis(15_000, "150.00"), ends_on: "2026-10-31", now: null, expected: null };

let calls: { url: string; method: string; body: unknown }[];

beforeEach(() => {
  calls = [];
  resetCsrf();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : null });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (url.startsWith("/api/budgets") && (init?.method ?? "GET") === "GET") {
        return new Response(JSON.stringify([FOOD, TRANSPORT, ENDED]));
      }
      if (url === "/api/categories") return new Response(JSON.stringify([]));
      if (url === "/api/accounts") return new Response(JSON.stringify([{ currency: "GHS" }]));
      return new Response(JSON.stringify(FOOD));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

describe("Tiers", () => {
  function cells(bps: number) {
    const { container } = render(<Tiers bps={bps} />);
    return {
      on: container.querySelectorAll(".tier.on").length,
      over: container.querySelectorAll(".tier.over").length,
      part: container.querySelectorAll(".tier.over--part").length,
      caps: container.querySelectorAll(".cap").length,
    };
  }

  it("fills a cell per tenth of the cap and draws the cap", () => {
    expect(cells(6_900)).toEqual({ on: 6, over: 0, part: 0, caps: 1 });
  });

  it("runs past the cap so 123% cannot look like 100%", () => {
    expect(cells(10_000)).toEqual({ on: 10, over: 0, part: 0, caps: 1 });
    expect(cells(12_300)).toEqual({ on: 10, over: 2, part: 1, caps: 1 });
  });

  it("stops running on at ten cells past the cap", () => {
    expect(cells(65_535)).toEqual({ on: 10, over: 10, part: 0, caps: 1 });
  });
});

describe("BudgetsScreen", () => {
  it("prints each standing as the server sent it", async () => {
    renderWithClient(<BudgetsScreen />);

    const list = await screen.findByRole("list", { name: "Budgets" });
    const [food, transport] = within(list).getAllByRole("listitem") as [HTMLElement, HTMLElement];
    expect(within(food).getByText("Over by 46.00")).toBeTruthy();
    expect(within(food).getByText(/123% of the limit used/)).toBeTruthy();
    expect(within(transport).getByText(/70\.00 left/)).toBeTruthy();
    // Fine now, and the expected rows say what they will do.
    expect(within(transport).getByText(/Planned payments this period take it to 90\.00 \(90%\) — close to the limit/)).toBeTruthy();
  });

  it("keeps ended caps apart, so history is not mistaken for a running cap", async () => {
    renderWithClient(<BudgetsScreen />);

    const earlier = await screen.findByRole("region", { name: "Earlier budgets" });
    expect(within(earlier).getByText(/150\.00 GHS a month, until/)).toBeTruthy();
  });

  it("sets a cap with the amount as typed and the reader's offset", async () => {
    const user = userEvent.setup();
    renderWithClient(<BudgetsScreen />);

    await user.type(await screen.findByLabelText("Limit"), "500");
    await user.click(screen.getByRole("button", { name: "Set budget" }));

    await waitFor(() => expect(calls.some((call) => call.method === "POST" && call.url === "/api/budgets")).toBe(true));
    const sent = calls.find((call) => call.method === "POST" && call.url === "/api/budgets");
    expect(sent?.body).toEqual({
      period: "monthly",
      cap: "500",
      currency: "GHS",
      warn_at_bps: 8_000,
      offset_minutes: -new Date().getTimezoneOffset() || 0,
    });
  });
});
