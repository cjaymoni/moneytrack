import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Recurring } from "../api/types";
import { resetCsrf } from "../api/client";
import { renderWithClient } from "../test-utils";
import { RecurringScreen, cadence } from "./RecurringScreen";

const RENT: Recurring = {
  id: "r-rent",
  account_id: "a1",
  account_name: "GCB Bank",
  direction: "out",
  amount: { minor_units: 140_000, currency: "GHS", display: "1400.00" },
  label: "Rent, Spintex",
  counterparty: null,
  category_id: null,
  freq: "monthly",
  interval: 1,
  starts_on: "2026-12-01",
  ends_on: null,
  next_on: "2026-12-01",
};

let calls: { url: string; method: string; body: unknown }[];

beforeEach(() => {
  calls = [];
  resetCsrf();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : null });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (url === "/api/recurring" && (init?.method ?? "GET") === "GET") return new Response(JSON.stringify([RENT]));
      if (url === "/api/accounts") return new Response(JSON.stringify([{ id: "a1", name: "GCB Bank", currency: "GHS" }]));
      if (url === "/api/categories") return new Response(JSON.stringify([]));
      return new Response(JSON.stringify(RENT));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

describe("cadence", () => {
  it("says how a series repeats as a person would", () => {
    expect(cadence("monthly", 1, "2026-12-01")).toBe("Every month on the 1st");
    expect(cadence("monthly", 3, "2026-12-22")).toBe("Every 3 months on the 22nd");
    expect(cadence("monthly", 1, "2027-01-31")).toBe("Every month on the 31st, or the last day of a shorter month");
    expect(cadence("weekly", 2, "2026-11-28")).toBe("Every 2 weeks on Saturday");
    expect(cadence("yearly", 1, "2027-01-15")).toBe("Every year on 15 January");
    expect(cadence("daily", 1, "2026-11-28")).toBe("Every day");
  });
});

describe("RecurringScreen", () => {
  it("lists each series with its cadence, account and next date", async () => {
    renderWithClient(<RecurringScreen />);

    expect(await screen.findByText("Rent, Spintex")).toBeTruthy();
    expect(screen.getByText(/Every month on the 1st · GCB Bank/)).toBeTruthy();
    expect(screen.getByText(/Next: Tuesday, 1 December 2026/)).toBeTruthy();
    expect(screen.getByText("−1,400.00")).toBeTruthy();
  });

  it("asks before stopping, and says what stopping does", async () => {
    const user = userEvent.setup();
    renderWithClient(<RecurringScreen />);

    await user.click(await screen.findByRole("button", { name: "Stop…" }));
    expect(screen.getByText(/future planned payments will be removed/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Stop it" }));

    await waitFor(() => expect(calls.some((call) => call.method === "DELETE" && call.url === "/api/recurring/r-rent")).toBe(true));
  });

  it("starts a series with the amount as typed and the reader's offset", async () => {
    const user = userEvent.setup();
    renderWithClient(<RecurringScreen />);

    await user.type(await screen.findByLabelText(/Amount each time/), "4200");
    await user.click(screen.getByText("Money in"));
    await user.type(screen.getByLabelText("Description"), "Salary");
    await user.click(screen.getByRole("button", { name: "Add regular payment" }));

    await waitFor(() => expect(calls.some((call) => call.method === "POST" && call.url === "/api/recurring")).toBe(true));
    const sent = calls.find((call) => call.method === "POST" && call.url === "/api/recurring")?.body as Record<string, unknown>;
    expect(sent).toMatchObject({
      account_id: "a1",
      direction: "in",
      amount: "4200",
      label: "Salary",
      freq: "monthly",
      interval: 1,
    });
    expect(sent.offset_minutes).toBe(-new Date().getTimezoneOffset() || 0);
  });
});
