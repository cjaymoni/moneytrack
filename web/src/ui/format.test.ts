import { describe, expect, it } from "vitest";
import { grouped, signed } from "./format";

const cedis = (display: string, minor_units: number) => ({ minor_units, currency: "GHS", display });

describe("grouped", () => {
  it("puts commas in the whole part and leaves the fraction alone", () => {
    expect(grouped("1400.00")).toBe("1,400.00");
    expect(grouped("1234567.89")).toBe("1,234,567.89");
    expect(grouped("999.99")).toBe("999.99");
  });

  it("uses a real minus sign", () => {
    expect(grouped("-12500.00")).toBe("−12,500.00");
  });

  it("groups a currency with no decimal places without inventing any", () => {
    // The CFA franc: mt-core writes no point, and this must not add one.
    expect(grouped("150000")).toBe("150,000");
  });

  it("hands back anything it does not recognise untouched", () => {
    expect(grouped("12,50")).toBe("12,50");
    expect(grouped("")).toBe("");
  });
});

describe("signed", () => {
  it("takes the sign from the direction and never from arithmetic", () => {
    expect(signed(cedis("4200.00", 420_000), "in")).toBe("+4,200.00");
    expect(signed(cedis("5.00", 500), "out")).toBe("−5.00");
  });
});
