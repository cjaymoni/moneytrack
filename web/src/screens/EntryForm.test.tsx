import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Account } from "../api/types";
import { resetCsrf } from "../api/client";
import { EntryForm } from "./EntryForm";

const account: Account = {
  id: "0190e1c2-0000-7000-8000-000000000001",
  name: "Wallet",
  kind: "momo",
  currency: "GHS",
  opening_balance: { minor_units: 0, currency: "GHS", display: "0.00" },
  confirmed: { minor_units: 0, currency: "GHS", display: "0.00" },
  projected: { minor_units: 0, currency: "GHS", display: "0.00" },
};

interface Call {
  url: string;
  init: RequestInit | undefined;
}

let calls: Call[];

beforeEach(() => {
  calls = [];
  resetCsrf();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok-123" }));
      if (url === "/api/transactions") {
        const sent = JSON.parse(String(init?.body)) as Record<string, unknown>;
        return new Response(
          JSON.stringify({
            id: "t1",
            account_id: sent.account_id,
            direction: sent.direction,
            amount: { minor_units: 1275, currency: "GHS", display: "12.75" },
            booked_at: "2026-11-20T12:00:00Z",
            category_id: null,
            label: sent.label,
            counterparty: null,
            note: null,
            status: "confirmed",
            source: "manual",
          }),
        );
      }
      return new Response("{}", { status: 404 });
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function renderForm() {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <EntryForm account={account} />
    </QueryClientProvider>,
  );
}

describe("EntryForm", () => {
  it("sends the amount as the text the person typed, with the CSRF token", async () => {
    const user = userEvent.setup();
    renderForm();

    await user.type(screen.getByLabelText(/Amount in GHS/), "12.75");
    await user.type(screen.getByLabelText("What it was"), "Waakye");
    await user.click(screen.getByRole("button", { name: "Record" }));

    await waitFor(() => expect(calls.some((call) => call.url === "/api/transactions")).toBe(true));
    const post = calls.find((call) => call.url === "/api/transactions");
    const headers = post?.init?.headers as Record<string, string>;
    expect(headers["x-csrf-token"]).toBe("tok-123");

    const body = JSON.parse(String(post?.init?.body)) as Record<string, unknown>;
    // Decimal text, read by mt-core on the server. Never multiplied into minor units here.
    expect(body.amount).toBe("12.75");
    expect(body).not.toHaveProperty("amount_minor");
    expect(body.direction).toBe("out");
    expect(body.status).toBe("confirmed");
    // Today is the server's own default, so no instant is sent for it.
    expect(body).not.toHaveProperty("booked_at");

    // Cleared for the next row.
    await waitFor(() => expect((screen.getByLabelText(/Amount in GHS/) as HTMLInputElement).value).toBe(""));
  });

  it("treats a day still to come as expected", async () => {
    const user = userEvent.setup();
    renderForm();

    const day = screen.getByLabelText("Day") as HTMLInputElement;
    await user.clear(day);
    await user.type(day, "2099-01-01");

    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(true);
  });
});
