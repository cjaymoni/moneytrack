import { describe, expect, it } from "vitest";
import { axisDay, axisMonth, dayName, tickLabel } from "./charts";

const money = (display: string, currency = "GHS") => ({ minor_units: 0, currency, display });

describe("tickLabel", () => {
  it("groups the amount and drops zeros that say nothing", () => {
    expect(tickLabel(money("200000.00"))).toBe("200,000");
    expect(tickLabel(money("0.00"))).toBe("0");
  });

  it("keeps a fraction that is not all zeros, and a currency with no fraction", () => {
    expect(tickLabel(money("2.50"))).toBe("2.50");
    expect(tickLabel(money("500", "XOF"))).toBe("500");
  });
});

describe("axisMonth", () => {
  it("names the month short, with the year on the first month and on January", () => {
    expect(axisMonth("2026-11", false)).toBe("Nov");
    expect(axisMonth("2026-11", true)).toBe("Nov 2026");
    expect(axisMonth("2027-01", false)).toBe("Jan 2027");
  });
});

describe("axisDay", () => {
  it("labels the first, every fifth and the thirtieth, and leaves the rest blank", () => {
    expect(["2026-11-01", "2026-11-02", "2026-11-05", "2026-11-30", "2026-12-31"].map(axisDay)).toEqual([
      "1",
      "",
      "5",
      "30",
      "",
    ]);
  });
});

describe("dayName", () => {
  it("spells out the date as written, whatever the reader's offset", () => {
    expect(dayName("2026-11-03")).toBe("Tuesday 3 November");
    expect(dayName("2027-01-01")).toBe("Friday 1 January");
  });
});
