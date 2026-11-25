import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import { renderInApp } from "../test-utils";
import { SettingsScreen } from "./SettingsScreen";

describe("SettingsScreen", () => {
  it("holds the theme, the way to categories, the help and the sign-in", () => {
    renderInApp(<SettingsScreen user={{ id: "u1", email: "ama@example.test", created_at: "2026-11-01T00:00:00Z" }} />);
    expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeTruthy();
    expect(screen.getByRole("link", { name: "Manage categories" }).getAttribute("href")).toBe("/categories");
    expect(screen.getByText("How moneytrack works")).toBeTruthy();
    expect(screen.getByText(/Signed in as ama@example\.test/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
  });
});
