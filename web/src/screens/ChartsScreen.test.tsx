import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import type { Breakdown, Daily, Money, Outlook, Trend, TrendMonth } from "../api/types";
import { renderWithClient } from "../test-utils";
import { ChartsScreen } from "./ChartsScreen";
import { monthOf, shiftMonth } from "./BreakdownScreen";

const cedis = (minor_units: number, display: string): Money => ({ minor_units, currency: "GHS", display });

const THIS_MONTH = monthOf(new Date());
const LAST_MONTH = shiftMonth(THIS_MONTH, -1);

function month(id: string, income: [number, string, number], spending: [number, string, number], net: string): TrendMonth {
  return {
    month: id,
    starts_on: `${id}-01`,
    ends_on: `${id}-28`,
    income: cedis(income[0], income[1]),
    spending: cedis(spending[0], spending[1]),
    net: cedis(net.startsWith("-") ? -1 : 1, net),
    income_level_bps: income[2],
    spending_level_bps: spending[2],
  };
}

const TREND: Trend = {
  currency: "GHS",
  months: [
    month(LAST_MONTH, [630_000, "6300.00", 7_875], [120_000, "1200.00", 1_500], "5100.00"),
    month(THIS_MONTH, [0, "0.00", 0], [315_000, "3150.00", 3_937], "-3150.00"),
  ],
  ticks: [0, 200_000, 400_000, 600_000, 800_000].map((minor, index) => ({
    amount: cedis(minor, `${minor / 100}.00`),
    level_bps: index * 2_500,
  })),
};

function line(id: string | null, name: string | null, display: string, bps: number, shown: string) {
  const share = { basis_points: bps, display: shown };
  const total = cedis(0, display);
  return { category_id: id, name, total, share, own: total, own_share: share, children: [] };
}

function breakdownFor(id: string): Breakdown {
  return {
    currency: "GHS",
    starts_on: `${id}-01`,
    ends_on: `${id}-28`,
    spending: {
      total: cedis(315_000, "3150.00"),
      lines:
        id === THIS_MONTH
          ? [line("c1", "Rent & housing", "2500.00", 7_936, "79%"), line(null, null, "650.00", 2_063, "20%")]
          : [line("c2", "Transport", "1200.00", 10_000, "100%")],
    },
    income: { total: cedis(0, "0.00"), lines: [] },
    net: cedis(-315_000, "-3150.00"),
  };
}

const OUTLOOK: Outlook = {
  account: {
    id: "a1",
    name: "MTN MoMo",
    kind: "momo",
    currency: "GHS",
    opening_balance: cedis(0, "0.00"),
    confirmed: cedis(399_700, "3997.00"),
    projected: cedis(399_700, "3997.00"),
  },
  from: "2026-11-23T00:00:00Z",
  until: "2027-02-21T00:00:00Z",
  start: cedis(399_700, "3997.00"),
  start_level_bps: 10_000,
  points: [],
  low: cedis(399_700, "3997.00"),
  low_transaction_id: null,
  high: cedis(399_700, "3997.00"),
  zero_level_bps: 0,
  shortfall: "none",
  shortfall_transaction_id: null,
};

function dailyFor(id: string): Daily {
  const days = Array.from({ length: 30 }, (_, index) => ({
    date: `${id}-${String(index + 1).padStart(2, "0")}`,
    spending: cedis(0, "0.00"),
    level_bps: 0,
  }));
  days[2] = { ...days[2]!, spending: cedis(2_500, "25.00"), level_bps: 1_666 };
  days[29] = { ...days[29]!, spending: cedis(12_000, "120.00"), level_bps: 8_000 };
  return {
    currency: "GHS",
    starts_on: `${id}-01`,
    ends_on: `${id}-30`,
    total: cedis(14_500, "145.00"),
    days,
    ticks: [0, 5_000, 10_000, 15_000].map((minor, index) => ({
      amount: cedis(minor, `${minor / 100}.00`),
      level_bps: Math.round((index * 10_000) / 3),
    })),
  };
}

let dailies: (id: string) => Daily[];
let trends: Trend[];
let requested: string[];

beforeEach(() => {
  trends = [TREND];
  dailies = (id) => [dailyFor(id)];
  requested = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      requested.push(url);
      const [path = url, query = ""] = url.split("?");
      const params = new URLSearchParams(query);
      if (path === "/api/trend") return new Response(JSON.stringify(trends));
      if (path === "/api/breakdown") return new Response(JSON.stringify([breakdownFor(params.get("month") ?? "")]));
      if (path === "/api/daily") return new Response(JSON.stringify(dailies(params.get("month") ?? "")));
      if (path === "/api/outlook") return new Response(JSON.stringify([OUTLOOK]));
      return new Response("{}", { status: 404 });
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function Where() {
  const location = useLocation();
  return <output data-testid="where">{location.search}</output>;
}

function renderCharts(path = "/charts") {
  return renderWithClient(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route
          path="/charts"
          element={
            <>
              <ChartsScreen />
              <Where />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );
}

describe("ChartsScreen", () => {
  it("asks for six months ending this month, on the reader's clock", async () => {
    renderCharts();
    await screen.findByRole("region", { name: "Money in and out" });
    const asked = requested.find((url) => url.startsWith("/api/trend"));
    expect(asked).toContain(`until=${THIS_MONTH}`);
    expect(asked).toContain("months=6");
    expect(asked).toMatch(/offset_minutes=-?\d+/);
  });

  it("draws each month's bars at the heights the server sent, and gridlines from its ticks", async () => {
    const { container } = renderCharts();
    await screen.findByRole("region", { name: "Money in and out" });

    const incomes = [...container.querySelectorAll<HTMLElement>(".trend__bar--in")].map((bar) => bar.style.height);
    const spends = [...container.querySelectorAll<HTMLElement>(".trend__bar--out")].map((bar) => bar.style.height);
    expect(incomes).toEqual(["78.75%", "0%"]);
    expect(spends).toEqual(["15%", "39.37%"]);
    const ticks = [...container.querySelectorAll(".trend__tick-label")].map((tick) => tick.textContent);
    expect(ticks).toEqual(["0", "2,000", "4,000", "6,000", "8,000"]);
  });

  it("names every month's figures for a screen reader, and keys the two colours in words", async () => {
    renderCharts();
    const chart = await screen.findByRole("region", { name: "Money in and out" });
    expect(within(chart).getByRole("button", { name: /money in 6,300\.00 GHS, money out 1,200\.00 GHS/ })).toBeTruthy();
    const key = within(chart).getByRole("list", { name: "Key" });
    expect(key.textContent).toBe("Money inMoney out");
  });

  it("has every figure in a table too, with a negative difference marked", async () => {
    renderCharts();
    const chart = await screen.findByRole("region", { name: "Money in and out" });
    const table = within(chart).getByRole("table");
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(rows[2]?.textContent).toMatch(/0\.003,150\.00−3,150\.00$/);
    expect(within(rows[2] as HTMLElement).getByText("−3,150.00").className).toBe("short");
  });

  it("shows this month's spending by category, each bar as wide as its share", async () => {
    const { container } = renderCharts();
    const bars = await screen.findByRole("list", { name: "Spending by category in GHS" });
    expect(within(bars).getByText("Rent & housing")).toBeTruthy();
    expect(within(bars).getByText("No category")).toBeTruthy();
    const widths = [...container.querySelectorAll<HTMLElement>(".ranked__fill")].map((bar) => bar.style.width);
    expect(widths).toEqual(["79.36%", "20.63%"]);
  });

  it("switches the category chart to a month you pick, and remembers it in the address", async () => {
    const user = userEvent.setup();
    renderCharts();
    const chart = await screen.findByRole("region", { name: "Money in and out" });
    const last = within(chart).getByRole("button", { name: /money in 6,300\.00/ });
    expect(last.getAttribute("aria-pressed")).toBe("false");

    await user.click(last);

    expect(last.getAttribute("aria-pressed")).toBe("true");
    expect(await screen.findByText("Transport")).toBeTruthy();
    expect(screen.getByTestId("where").textContent).toBe(`?month=${LAST_MONTH}`);
  });

  it("asks for twelve months when you pick twelve", async () => {
    const user = userEvent.setup();
    renderCharts();
    await screen.findByRole("region", { name: "Money in and out" });
    await user.click(screen.getByRole("radio", { name: "12 months" }));
    expect(screen.getByTestId("where").textContent).toBe("?months=12");
    expect(requested.some((url) => url.includes("months=12"))).toBe(true);
  });

  it("ignores a month in the address that is in the future or will not read", async () => {
    renderCharts("/charts?month=2999-01");
    expect(await screen.findByRole("heading", { name: /Where the money went in/ })).toBeTruthy();
    expect(requested.some((url) => url.includes(`/api/breakdown?month=${THIS_MONTH}`))).toBe(true);
  });

  it("says so when nothing moved, rather than drawing an empty frame", async () => {
    trends = [
      {
        ...TREND,
        months: TREND.months.map((each) => ({ ...each, income_level_bps: 0, spending_level_bps: 0 })),
        ticks: [{ amount: cedis(0, "0.00"), level_bps: 0 }],
      },
    ];
    renderCharts();
    expect(await screen.findByText("Nothing came in or went out in these months.")).toBeTruthy();
  });

  it("draws the picked month day by day, at the heights the server sent", async () => {
    const { container } = renderCharts();
    const block = await screen.findByRole("region", { name: /Spending day by day in/ });
    expect(await within(block).findByText(/145\.00 GHS in all, on 2 days\. The most was 120\.00 on/)).toBeTruthy();
    const bars = [...container.querySelectorAll<HTMLElement>(".trend--days .trend__bar")];
    expect(bars).toHaveLength(30);
    expect(bars[2]?.style.height).toBe("16.66%");
    expect(bars[29]?.style.height).toBe("80%");
    const labels = [...container.querySelectorAll(".trend--days .trend__label")].map((label) => label.textContent);
    expect(labels.filter(Boolean)).toEqual(["1", "5", "10", "15", "20", "25", "30"]);
  });

  it("lists only the days something was spent in the day-by-day table", async () => {
    renderCharts();
    const block = await screen.findByRole("region", { name: /Spending day by day in/ });
    const table = await within(block).findByRole("table");
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(rows[2]?.textContent).toMatch(/120\.00$/);
  });

  it("follows the month you pick in the chart above", async () => {
    const user = userEvent.setup();
    renderCharts();
    const chart = await screen.findByRole("region", { name: "Money in and out" });
    await user.click(within(chart).getByRole("button", { name: /money in 6,300\.00/ }));
    expect(await screen.findByRole("heading", { name: /Spending day by day in .*/ })).toBeTruthy();
    expect(requested.some((url) => url.startsWith(`/api/daily?month=${LAST_MONTH}`))).toBe(true);
  });

  it("says so when nothing was spent that month", async () => {
    dailies = (id) => [{ ...dailyFor(id), days: dailyFor(id).days.map((day) => ({ ...day, spending: cedis(0, "0.00"), level_bps: 0 })) }];
    renderCharts();
    expect(await screen.findByText("Nothing spent in GHS this month.")).toBeTruthy();
  });

  it("draws each account's balance line with its lowest point in words", async () => {
    renderCharts();
    expect((await screen.findByText("MTN MoMo")).closest("p")?.textContent).toMatch(
      /now 3,997\.00 GHS, lowest 3,997\.00 GHS today/,
    );
  });

  it("points someone with no accounts at adding one", async () => {
    trends = [];
    renderCharts();
    expect(await screen.findByRole("link", { name: "Add an account" })).toBeTruthy();
  });
});
