import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Account, CountResult } from "../api/types";
import { resetCsrf } from "../api/client";
import { renderWithClient } from "../test-utils";
import { CountIt } from "./CountIt";

const cedis = (minor_units: number, display: string) => ({ minor_units, currency: "GHS", display });

const CASH: Account = {
  id: "a-cash",
  name: "Cash",
  kind: "cash",
  currency: "GHS",
  opening_balance: cedis(38_000, "380.00"),
  confirmed: cedis(38_000, "380.00"),
  projected: cedis(38_000, "380.00"),
};

const CORRECTED: CountResult = {
  ledger: cedis(38_000, "380.00"),
  counted: cedis(34_000, "340.00"),
  adjustment: {
    id: "t-adj",
    account_id: "a-cash",
    direction: "out",
    amount: cedis(4_000, "40.00"),
    booked_at: "2026-11-23T10:00:00Z",
    category_id: null,
    label: "Counted: 340.00 GHS",
    counterparty: null,
    note: null,
    status: "confirmed",
    source: "adjustment",
    recurring_id: null,
  },
};

let answer: CountResult;
let calls: { url: string; method: string; body: unknown }[];

beforeEach(() => {
  calls = [];
  answer = CORRECTED;
  resetCsrf();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : null });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (url.endsWith("/reconcile")) return new Response(JSON.stringify(answer));
      return new Response(JSON.stringify({ deleted: true }));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

async function countAs(text: string) {
  const user = userEvent.setup();
  renderWithClient(<CountIt account={CASH} />);
  await user.click(screen.getByRole("button", { name: "Count it" }));
  await user.type(screen.getByLabelText(/What it actually holds now/), text);
  await user.click(screen.getByRole("button", { name: "Make it agree" }));
  return user;
}

describe("CountIt", () => {
  it("sends the count as typed and says what the server recorded", async () => {
    await countAs("340");

    expect(await screen.findByText(/The ledger said 380\.00; you counted 340\.00/)).toBeTruthy();
    expect(screen.getByText("−40.00")).toBeTruthy();
    const sent = calls.find((call) => call.url === "/api/accounts/a-cash/reconcile");
    expect(sent?.body).toEqual({ counted: "340" });
  });

  it("says so when the count agrees, and records nothing", async () => {
    answer = { ...CORRECTED, counted: cedis(38_000, "380.00"), adjustment: null };
    await countAs("380");

    expect(await screen.findByText(/It agrees: the ledger also says 380\.00 GHS/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("takes a correction back by deleting the row it wrote", async () => {
    const user = await countAs("340");

    await user.click(await screen.findByRole("button", { name: "Undo" }));

    await waitFor(() => expect(screen.getByText(/Taken back/)).toBeTruthy());
    expect(calls.some((call) => call.method === "DELETE" && call.url === "/api/transactions/t-adj")).toBe(true);
  });
});
