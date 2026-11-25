import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Category, Transaction } from "../api/types";
import { resetCsrf } from "../api/client";
import { renderWithClient } from "../test-utils";
import { RowEditor } from "./RowEditor";

const BOOKED = new Date(2026, 10, 10, 7, 40).toISOString();

const ROW: Transaction = {
  id: "t1",
  account_id: "a1",
  direction: "out",
  amount: { minor_units: 1250, currency: "GHS", display: "12.50" },
  booked_at: BOOKED,
  category_id: "c-food",
  label: "Waakye",
  counterparty: "Auntie Muni",
  note: null,
  status: "confirmed",
  source: "manual",
  recurring_id: null,
};

const CATEGORIES: Category[] = [
  { id: "c-food", name: "Food & groceries", parent_id: null, kind: "expense", sort_order: 0 },
  { id: "c-salary", name: "Salary", parent_id: null, kind: "income", sort_order: 1 },
];

interface Call {
  url: string;
  method: string;
  body: Record<string, unknown> | null;
  token: string | undefined;
}

let calls: Call[];

beforeEach(() => {
  calls = [];
  resetCsrf();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const headers = (init?.headers ?? {}) as Record<string, string>;
      calls.push({
        url,
        method: init?.method ?? "GET",
        body: init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : null,
        token: headers["x-csrf-token"],
      });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (init?.method === "PATCH") return new Response(JSON.stringify({ ...ROW, ...JSON.parse(String(init.body)) }));
      if (init?.method === "DELETE") return new Response(JSON.stringify({ deleted: true }));
      return new Response("{}", { status: 404 });
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function renderEditor() {
  const onDone = vi.fn();
  const onDeleted = vi.fn();
  renderWithClient(
    <RowEditor row={ROW} categories={CATEGORIES} onDone={onDone} onDeleted={onDeleted} />,
  );
  return { onDone, onDeleted };
}

describe("RowEditor", () => {
  it("starts from the row as the server rendered it", () => {
    renderEditor();

    expect((screen.getByLabelText(/Amount, GHS/) as HTMLInputElement).value).toBe("12.50");
    expect((screen.getByLabelText("Description") as HTMLInputElement).value).toBe("Waakye");
    expect(screen.getByRole("button", { name: "Food & groceries" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("sends the whole row back, the amount as text, the time of day kept", async () => {
    const user = userEvent.setup();
    const { onDone } = renderEditor();

    const amount = screen.getByLabelText(/Amount, GHS/);
    await user.clear(amount);
    await user.type(amount, "15");
    await user.clear(screen.getByLabelText("Paid to"));
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onDone).toHaveBeenCalled());
    const sent = calls.find((call) => call.method === "PATCH");
    expect(sent?.url).toBe("/api/transactions/t1");
    expect(sent?.token).toBe("tok");
    expect(sent?.body).toEqual({
      direction: "out",
      amount: "15",
      booked_at: BOOKED,
      label: "Waakye",
      counterparty: null,
      note: null,
      status: "confirmed",
      category_id: "c-food",
    });
  });

  it("clears a spending category when the row turns into money in", async () => {
    const user = userEvent.setup();
    renderEditor();

    await user.click(screen.getByText("Money in"));
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    const sent = calls.find((call) => call.method === "PATCH");
    expect(sent?.body?.direction).toBe("in");
    expect(sent?.body?.category_id).toBeNull();
  });

  it("deletes the row and hands it back for an undo", async () => {
    const user = userEvent.setup();
    const { onDeleted } = renderEditor();

    await user.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(onDeleted).toHaveBeenCalledWith(ROW));
    const sent = calls.find((call) => call.method === "DELETE");
    expect(sent?.url).toBe("/api/transactions/t1");
    expect(sent?.token).toBe("tok");
  });
});
