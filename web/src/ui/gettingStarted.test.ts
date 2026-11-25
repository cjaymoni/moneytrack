import { describe, expect, it, vi } from "vitest";
import { HIDDEN_KEY, isComplete, readHidden, saveHidden, steps } from "./gettingStarted";

describe("steps", () => {
  it("marks each step from what the person has, in order", () => {
    expect(steps({ hasAccount: true, hasTransaction: false, hasBudget: false }).map((step) => step.done)).toEqual([
      true,
      false,
      false,
    ]);
  });

  it("is complete only when every step is done", () => {
    expect(isComplete({ hasAccount: true, hasTransaction: true, hasBudget: false })).toBe(false);
    expect(isComplete({ hasAccount: true, hasTransaction: true, hasBudget: true })).toBe(true);
  });
});

describe("hiding the checklist", () => {
  it("remembers being hidden", () => {
    const store = new Map<string, string>();
    const storage = { getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => void store.set(key, value) };
    expect(readHidden(storage)).toBe(false);
    saveHidden(storage);
    expect(store.get(HIDDEN_KEY)).toBe("1");
    expect(readHidden(storage)).toBe(true);
  });

  it("shows the checklist when storage is unavailable, and says why", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(readHidden(broken)).toBe(false);
    saveHidden(broken);
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });
});
