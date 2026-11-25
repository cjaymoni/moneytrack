import { describe, expect, it } from "vitest";
import type { Category } from "../api/types";
import { fullName, ordered, visible } from "./CategoryPicker";

const c = (id: string, name: string, kind: "income" | "expense", parent_id: string | null = null): Category => ({
  id,
  name,
  kind,
  parent_id,
  sort_order: 0,
});

describe("ordered", () => {
  const all = [
    c("food", "Food", "expense"),
    c("transport", "Transport", "expense"),
    c("trotro", "Trotro", "expense", "transport"),
    c("chop", "Chop bar", "expense", "food"),
    c("salary", "Salary", "income"),
    c("orphan", "Lost", "expense", "gone"),
  ];

  it("puts each child straight after its parent, and keeps to one side", () => {
    expect(ordered(all, "expense").map((each) => each.id)).toEqual([
      "food",
      "chop",
      "transport",
      "trotro",
      "orphan",
    ]);
    expect(ordered(all, "income").map((each) => each.id)).toEqual(["salary"]);
  });

  it("names a child after its parent too", () => {
    expect(fullName(all[2]!, all)).toBe("Transport · Trotro");
    expect(fullName(all[5]!, all)).toBe("Lost");
  });
});

describe("visible", () => {
  const many = Array.from({ length: 10 }, (_, index) => ({
    id: `c${index}`,
    name: `Category ${index}`,
    parent_id: null,
    kind: "expense" as const,
    sort_order: index,
  }));

  it("shows the first few, and everything when no limit is set or the list is short", () => {
    expect(visible(many, null, 3).map((each) => each.id)).toEqual(["c0", "c1", "c2"]);
    expect(visible(many, null)).toHaveLength(10);
    expect(visible(many.slice(0, 2), null, 3)).toHaveLength(2);
  });

  it("never hides the chosen category", () => {
    expect(visible(many, "c8", 3).map((each) => each.id)).toEqual(["c0", "c1", "c2", "c8"]);
    expect(visible(many, "c1", 3).map((each) => each.id)).toEqual(["c0", "c1", "c2"]);
  });
});
