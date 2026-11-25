import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetCsrf } from "../api/client";
import { renderWithClient } from "../test-utils";
import { CategoriesScreen } from "./CategoriesScreen";

const CATEGORIES = [
  { id: "c-food", name: "Food & groceries", parent_id: null, kind: "expense", sort_order: 0 },
  { id: "c-eat", name: "Eating out", parent_id: null, kind: "expense", sort_order: 1 },
  { id: "c-salary", name: "Salary", parent_id: null, kind: "income", sort_order: 2 },
];

let sent: { method: string; url: string; body?: unknown }[];

beforeEach(() => {
  resetCsrf();
  sent = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (url === "/api/categories" && method === "GET") return new Response(JSON.stringify(CATEGORIES));
      sent.push({ method, url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
      if (method === "DELETE") return new Response(null, { status: 204 });
      return new Response(JSON.stringify({ ...CATEGORIES[1], name: "Chop bars" }));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

describe("CategoriesScreen", () => {
  it("lists the categories first, with the add form a click away", async () => {
    const user = userEvent.setup();
    renderWithClient(<CategoriesScreen />);
    expect((await screen.findAllByText("Eating out")).length).toBeGreaterThan(0);
    expect(screen.queryByRole("form", { name: "Add a category" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "Add a category" }));

    expect(screen.getByRole("form", { name: "Add a category" })).toBeTruthy();
  });

  it("renames a category in place", async () => {
    const user = userEvent.setup();
    renderWithClient(<CategoriesScreen />);
    await user.click(await screen.findByRole("button", { name: "Rename Eating out" }));
    const form = within(screen.getByRole("form", { name: "Rename Eating out" }));
    await user.clear(form.getByRole("textbox"));
    await user.type(form.getByRole("textbox"), "Chop bars");

    await user.click(form.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(sent).toHaveLength(1));
    expect(sent[0]).toEqual({ method: "PATCH", url: "/api/categories/c-eat", body: { name: "Chop bars" } });
  });

  it("asks where a deleted category's transactions go, offering only the same side", async () => {
    const user = userEvent.setup();
    renderWithClient(<CategoriesScreen />);
    await user.click(await screen.findByRole("button", { name: "Delete Eating out" }));
    const form = within(screen.getByRole("form", { name: "Delete Eating out" }));
    const options = form.getAllByRole("option").map((option) => option.textContent);
    expect(options).toEqual(["No category", "Food & groceries"]);

    await user.selectOptions(form.getByRole("combobox"), "c-food");
    await user.click(form.getByRole("button", { name: "Delete Eating out" }));

    await waitFor(() => expect(sent).toHaveLength(1));
    expect(sent[0]).toMatchObject({ method: "DELETE", url: "/api/categories/c-eat?move_to=c-food" });
  });

  it("lets the person back out of deleting", async () => {
    const user = userEvent.setup();
    renderWithClient(<CategoriesScreen />);
    await user.click(await screen.findByRole("button", { name: "Delete Salary" }));
    await user.click(screen.getByRole("button", { name: "Keep it" }));
    expect(screen.queryByRole("form", { name: "Delete Salary" })).toBeNull();
    expect(sent).toEqual([]);
  });
});
