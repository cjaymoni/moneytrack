import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import type { Breakdown } from "../api/types";
import { renderWithClient } from "../test-utils";
import { BreakdownScreen, monthName, shiftMonth } from "./BreakdownScreen";

const cedis = (minor_units: number, display: string) => ({ minor_units, currency: "GHS", display });
const share = (basis_points: number, display: string) => ({ basis_points, display });

const NOVEMBER: Breakdown = {
  currency: "GHS",
  starts_on: "2026-11-01",
  ends_on: "2026-11-30",
  spending: {
    total: cedis(1_210_000, "12100.00"),
    lines: [
      {
        category_id: "c-transport",
        name: "Transport",
        // Deliberately not the sum of its parts plus its own: if the screen ever added
        // them up instead of printing this, the test would see the sum.
        total: cedis(999_900, "9999.00"),
        share: share(6_666, "66%"),
        own: cedis(100, "1.00"),
        own_share: share(1, "<1%"),
        children: [
          {
            category_id: "c-trotro",
            name: "Trotro",
            total: cedis(400, "4.00"),
            share: share(3_333, "33%"),
          },
        ],
      },
      {
        category_id: null,
        name: null,
        total: cedis(500, "5.00"),
        share: share(0, "0%"),
        own: cedis(500, "5.00"),
        own_share: share(0, "0%"),
        children: [],
      },
    ],
  },
  income: { total: cedis(0, "0.00"), lines: [] },
  net: cedis(-1_210_000, "-12100.00"),
};

let asked: string[];

beforeEach(() => {
  asked = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      asked.push(url);
      return new Response(JSON.stringify(url.includes("month=2026-11") ? [NOVEMBER] : []));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function renderAt(path: string) {
  renderWithClient(
    <MemoryRouter initialEntries={[path]}>
      <BreakdownScreen />
    </MemoryRouter>,
  );
}

describe("BreakdownScreen", () => {
  it("prints the server's totals and shares, and draws bars from the share alone", async () => {
    renderAt("/breakdown?month=2026-11");

    // The headline and the section's own total are both the server's figure.
    expect(await screen.findAllByText("12,100.00")).toHaveLength(2);
    expect(screen.getByText("−12,100.00")).toBeTruthy();

    const spending = screen.getByRole("region", { name: "Spending" });
    expect(within(spending).getByText("9,999.00")).toBeTruthy();
    expect(within(spending).getByText("66%")).toBeTruthy();
    expect(within(spending).getByText("Trotro")).toBeTruthy();
    expect(within(spending).getByText("Transport, not further filed")).toBeTruthy();
    expect(within(spending).getByText("Uncategorised")).toBeTruthy();

    const widths = [...spending.querySelectorAll<HTMLElement>(".bar__fill")].map(
      (fill) => fill.style.width,
    );
    expect(widths).toEqual(["66.66%", "33.33%", "0.01%", "0%"]);
  });

  it("sends the reader's offset so a late row lands in their own month", async () => {
    renderAt("/breakdown?month=2026-11");
    await screen.findAllByText("12,100.00");

    const offset = -new Date().getTimezoneOffset();
    expect(asked[0]).toBe(`/api/breakdown?month=2026-11&offset_minutes=${offset}`);
  });

  it("says when a month has nothing in it", async () => {
    renderAt("/breakdown?month=2026-10");

    expect(await screen.findByText(/Open an account/)).toBeTruthy();
  });
});

describe("months", () => {
  it("steps across a year's edge", () => {
    expect(shiftMonth("2026-12", 1)).toBe("2027-01");
    expect(shiftMonth("2026-01", -1)).toBe("2025-12");
  });

  it("names a month as a person would say it", () => {
    expect(monthName("2026-11")).toBe("November 2026");
  });
});
