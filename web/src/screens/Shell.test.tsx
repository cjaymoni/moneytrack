import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import type { User } from "../api/types";
import { renderWithClient } from "../test-utils";
import { Shell } from "./Shell";

const ACCOUNT = {
  id: "a1",
  name: "MTN MoMo",
  kind: "momo",
  currency: "GHS",
  opening_balance: { minor_units: 0, currency: "GHS", display: "0.00" },
  confirmed: { minor_units: 100, currency: "GHS", display: "1.00" },
  projected: { minor_units: 100, currency: "GHS", display: "1.00" },
};

const USER: User = { id: "u1", email: "ama@example.test", created_at: "2026-11-01T00:00:00Z" };

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const path = url.split("?")[0];
      if (path === "/api/inbox/count") return new Response(JSON.stringify({ total: 2, by_account: [] }));
      if (path === "/api/accounts") return new Response(JSON.stringify([ACCOUNT]));
      if (path === "/api/transactions") {
        return new Response(JSON.stringify([{ id: "t1", category_id: null }, { id: "t2", category_id: "c" }]));
      }
      return new Response("[]");
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

function renderShell(path = "/") {
  return renderWithClient(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route element={<Shell user={USER} />}>
          <Route index element={<h1>Home page</h1>} />
          <Route path="budgets" element={<h1>Budgets page</h1>} />
          <Route path="charts" element={<h1>Trends page</h1>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

describe("Shell", () => {
  it("offers a way past the menu, straight to the page", () => {
    renderShell();
    const skip = screen.getByRole("link", { name: "Skip to content" });
    expect(skip.getAttribute("href")).toBe("#content");
    expect(screen.getByRole("main").id).toBe("content");
  });

  it("folds the menu behind a button that says whether it is open", async () => {
    const user = userEvent.setup();
    renderShell();
    const toggle = screen.getByRole("button", { name: "Menu" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(toggle.getAttribute("aria-controls")).toBe("main-menu");

    await user.click(toggle);

    expect(screen.getByRole("button", { name: "Close menu" }).getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("navigation", { name: "Main" }).className).toContain("rail--open");
  });

  it("closes the menu and moves focus to the page after choosing where to go", async () => {
    const user = userEvent.setup();
    renderShell();
    await user.click(screen.getByRole("button", { name: "Menu" }));

    await user.click(screen.getByRole("link", { name: "Budgets" }));

    expect(await screen.findByText("Budgets page")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Menu" }).getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(screen.getByRole("main"));
  });

  it("lists the chores with counts, then the accounts, then the rest", async () => {
    renderShell();
    const menu = screen.getByRole("navigation", { name: "Main" });
    await screen.findByText("MTN MoMo");
    const order = [...menu.querySelectorAll("a")].map((link) => link.textContent?.replace(/\s+/g, " ").trim());
    expect(order.slice(0, 4)).toEqual([
      "Home",
      "Check imports2 waiting",
      "Sort into categories1 without a category",
      "MTN MoMoMobile money1.00GHS",
    ]);
    expect(order).toContain("Insights");
    expect(order[order.length - 1]).toBe("Settings");
  });

  it("marks Insights as the current place on any of its three pages", async () => {
    renderShell("/charts");
    expect((await screen.findByRole("link", { name: "Insights" })).getAttribute("aria-current")).toBe("page");
  });

  it("gives every place a row with its count, amber only for imports waiting", async () => {
    renderShell();
    const check = await screen.findByRole("link", { name: /Check imports/ });
    await waitFor(() => expect(check.querySelector(".rail__badge--urgent")?.textContent).toBe("2 waiting"));
    const sort = screen.getByRole("link", { name: /Sort into categories/ });
    await waitFor(() => expect(sort.querySelector(".rail__badge")?.textContent).toBe("1 without a category"));
    expect(sort.querySelector(".rail__badge--urgent")).toBeNull();
    expect(screen.getByRole("link", { name: "Home" }).querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  });
});
