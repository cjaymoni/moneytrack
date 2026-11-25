import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { APP_NAME, pageTitle, usePageTitle } from "./title";

function Page({ name }: { name?: string }) {
  usePageTitle(name);
  return null;
}

describe("pageTitle", () => {
  it("puts the page before the app, and falls back to the app alone", () => {
    expect(pageTitle("Home")).toBe(`Home · ${APP_NAME}`);
    expect(pageTitle("  ")).toBe(APP_NAME);
    expect(pageTitle(undefined)).toBe(APP_NAME);
  });
});

describe("usePageTitle", () => {
  it("names the tab while the page is shown and restores it after", () => {
    const { rerender, unmount } = render(<Page name="Budgets" />);
    expect(document.title).toBe(`Budgets · ${APP_NAME}`);
    rerender(<Page name="MTN MoMo" />);
    expect(document.title).toBe(`MTN MoMo · ${APP_NAME}`);
    unmount();
    expect(document.title).toBe(APP_NAME);
  });
});
