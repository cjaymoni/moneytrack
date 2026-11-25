import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import type { Account, Ledger, Money } from "../api/types";
import { renderWithClient } from "../test-utils";
import { AccountScreen } from "./AccountScreen";

const cedis = (minor_units: number, display: string): Money => ({ minor_units, currency: "GHS", display });

const ACCOUNT: Account = {
  id: "a1",
  name: "MTN MoMo",
  kind: "momo",
  currency: "GHS",
  opening_balance: cedis(0, "0.00"),
  confirmed: cedis(5_000, "50.00"),
  projected: cedis(5_000, "50.00"),
};

const ROW = {
  transaction: {
    id: "t1",
    account_id: "a1",
    direction: "out" as const,
    amount: cedis(1_000, "10.00"),
    booked_at: "2026-11-02T09:00:00Z",
    category_id: null,
    label: "Kenkey",
    counterparty: null,
    note: null,
    status: "confirmed" as const,
    source: "manual",
    recurring_id: null,
  },
  balance_after: cedis(5_000, "50.00"),
};

let entries: Ledger["entries"];

beforeEach(() => {
  entries = [ROW];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const path = url.split("?")[0];
      if (path === "/api/ledger") {
        const ledger: Ledger = {
          account: ACCOUNT,
          basis: "projected",
          opening: cedis(6_000, "60.00"),
          entries,
          closing: cedis(5_000, "50.00"),
        };
        return new Response(JSON.stringify(ledger));
      }
      if (path === "/api/categories") return new Response("[]");
      if (path === "/api/inbox/count") return new Response(JSON.stringify({ total: 0, by_account: [] }));
      return new Response("{}", { status: 404 });
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function renderAccount(path = "/accounts/a1") {
  return renderWithClient(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/accounts/:accountId" element={<AccountScreen />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("AccountScreen", () => {
  it("leads with the balance and the two actions, and opens the form when asked", async () => {
    const user = userEvent.setup();
    renderAccount();
    await screen.findByText("Kenkey");
    expect(screen.queryByRole("form", { name: "Add a transaction" })).toBeNull();
    expect(screen.getByRole("button", { name: "Balance wrong? Correct it" })).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Add a transaction" }));

    expect(screen.getByRole("form", { name: "Add a transaction" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("form", { name: "Add a transaction" })).toBeNull();
  });

  it("opens the form straight away when sent here to add one", async () => {
    renderAccount("/accounts/a1?add=1");
    expect(await screen.findByRole("form", { name: "Add a transaction" })).toBeTruthy();
  });

  it("opens the form on an empty account, with nothing to close it into", async () => {
    entries = [];
    renderAccount();
    expect(await screen.findByRole("form", { name: "Add a transaction" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Done" })).toBeNull();
  });
});
