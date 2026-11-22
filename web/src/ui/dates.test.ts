import { describe, expect, it } from "vitest";
import { keptOrMoved, localDateOf, middayOn } from "./dates";

describe("keptOrMoved", () => {
  it("keeps the original instant when the day was not changed", () => {
    // The time of day matters: it orders two rows on one day in the walk.
    const original = new Date(2026, 10, 10, 7, 40).toISOString();
    expect(keptOrMoved(original, localDateOf(original))).toBe(original);
  });

  it("moves to midday on a new day, on the reader's clock", () => {
    const original = new Date(2026, 10, 10, 7, 40).toISOString();
    const moved = keptOrMoved(original, "2026-11-12");
    expect(moved).toBe(new Date(2026, 10, 12, 12, 0).toISOString());
    expect(localDateOf(moved)).toBe("2026-11-12");
  });

  it("keeps the original rather than guessing at a day that will not read", () => {
    const original = new Date(2026, 10, 10, 7, 40).toISOString();
    expect(keptOrMoved(original, "")).toBe(original);
    expect(middayOn("not a day")).toBeUndefined();
  });
});
