import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ImportOutcome, ImportPreview } from "../api/types";
import { resetCsrf } from "../api/client";
import { renderInApp } from "../test-utils";
import { ImportScreen } from "./ImportScreen";

const STATEMENT = "Date,Description,Amount,Payee\n03/10/2026,POS PURCHASE,-120.50,SHOPRITE\n";

const PREVIEW: ImportPreview = {
  delimiter: "comma",
  rows: [
    ["Date", "Description", "Amount", "Payee"],
    ["03/10/2026", "POS PURCHASE", "-120.50", "SHOPRITE"],
  ],
  saved_mapping: null,
};

const OUTCOME: ImportOutcome = {
  import_id: "imp-1",
  read: 3,
  added: 2,
  already_seen: 1,
  problem_count: 1,
  problems: [{ line_no: 5, reason: "`Closing balance` is not a date in the chosen format" }],
};

let calls: { url: string; method: string; body: Record<string, unknown> | null }[];

beforeEach(() => {
  calls = [];
  resetCsrf();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : null });
      if (url === "/api/csrf") return new Response(JSON.stringify({ token: "tok" }));
      if (url === "/api/accounts") return new Response(JSON.stringify([{ id: "a1", name: "GCB Bank", currency: "GHS" }]));
      if (url === "/api/imports/preview") return new Response(JSON.stringify(PREVIEW));
      if (url === "/api/imports") return new Response(JSON.stringify(OUTCOME));
      return new Response(JSON.stringify({}));
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

const sent = (url: string) => calls.find((call) => call.method === "POST" && call.url === url)?.body;

describe("ImportScreen", () => {
  it("previews a CSV, guesses its columns, and sends the file whole with the mapping", async () => {
    const user = userEvent.setup();
    renderInApp(<ImportScreen />);

    await user.upload(
      await screen.findByLabelText("Statement file"),
      new File([STATEMENT], "oct.csv", { type: "text/csv" }),
    );

    // The reading comes first, as a table; the column settings are folded beneath it.
    const reading = await screen.findByRole("table", { name: "How the first transactions will be read" });
    expect(within(reading).getAllByRole("row")[1]?.textContent).toMatch(/03\/10\/2026POS PURCHASE-120\.50SHOPRITE/);
    expect(screen.getByText("Fix how it's read")).toBeTruthy();
    expect((screen.getByLabelText("Description") as HTMLSelectElement).value).toBe("1");
    expect((screen.getByLabelText("Payee") as HTMLSelectElement).value).toBe("3");
    expect(sent("/api/imports/preview")).toMatchObject({ account_id: "a1", content_base64: btoa(STATEMENT) });

    await user.click(screen.getByRole("button", { name: "Looks right — import" }));

    await waitFor(() => expect(sent("/api/imports")).toBeTruthy());
    expect(sent("/api/imports")).toMatchObject({
      account_id: "a1",
      file_name: "oct.csv",
      format: "csv",
      content_base64: btoa(STATEMENT),
      mapping: {
        has_header: true,
        skip_rows: 0,
        date: 0,
        label: 1,
        amount: { kind: "signed", column: 2 },
        counterparty: 3,
        date_format: "day_month_year",
      },
    });
  });

  it("says what came in, what was seen before, and which lines were left out", async () => {
    const user = userEvent.setup();
    renderInApp(<ImportScreen />);
    await user.upload(await screen.findByLabelText("Statement file"), new File([STATEMENT], "oct.csv"));
    await screen.findByRole("table", { name: "How the first transactions will be read" });

    await user.click(screen.getByRole("button", { name: "Looks right — import" }));

    expect(await screen.findByText(/2 are new/)).toBeTruthy();
    expect(screen.getByRole("link", { name: "Review them now" })).toBeTruthy();
    expect(screen.getByText(/1 was already imported/)).toBeTruthy();
    expect(screen.getByText(/Line 5: `Closing balance` is not a date/)).toBeTruthy();
  });

  it("sends an OFX file without a mapping or a preview", async () => {
    const user = userEvent.setup();
    renderInApp(<ImportScreen />);

    await user.upload(await screen.findByLabelText("Statement file"), new File(["<OFX></OFX>"], "oct.ofx"));
    expect(screen.getByText(/OFX files need no setup/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Import" }));

    await waitFor(() => expect(sent("/api/imports")).toBeTruthy());
    expect(sent("/api/imports")).toMatchObject({ format: "ofx", file_name: "oct.ofx" });
    expect(sent("/api/imports")).not.toHaveProperty("mapping");
    expect(sent("/api/imports/preview")).toBeUndefined();
  });

  it("refuses a file over the size limit before uploading it", async () => {
    const user = userEvent.setup();
    renderInApp(<ImportScreen />);
    const huge = new File(["x"], "year.csv");
    Object.defineProperty(huge, "size", { value: 6 * 1024 * 1024 });

    await user.upload(await screen.findByLabelText("Statement file"), huge);

    expect(screen.getByText(/over 5 MB/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Import" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
