import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { THEME_KEY, applyTheme, readTheme, saveTheme } from "./theme";
import { ThemeSwitcher } from "./ThemeSwitcher";

const root = document.documentElement;
const broken = {
  getItem: () => {
    throw new Error("blocked");
  },
  setItem: () => {
    throw new Error("blocked");
  },
  removeItem: () => {
    throw new Error("blocked");
  },
};

beforeEach(() => {
  localStorage.clear();
  delete root.dataset.theme;
});
afterEach(() => vi.restoreAllMocks());

describe("readTheme", () => {
  it("follows the device when nothing is saved", () => {
    expect(readTheme()).toBe("system");
  });

  it("returns a saved light or dark", () => {
    localStorage.setItem(THEME_KEY, "dark");
    expect(readTheme()).toBe("dark");
  });

  it("ignores a saved value it does not know", () => {
    localStorage.setItem(THEME_KEY, "sepia");
    expect(readTheme()).toBe("system");
  });

  it("follows the device, and says so, when storage cannot be read", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(readTheme(broken)).toBe("system");
    expect(warn).toHaveBeenCalledOnce();
  });
});

describe("saveTheme", () => {
  it("keeps light or dark and forgets the key for auto", () => {
    saveTheme("light");
    expect(localStorage.getItem(THEME_KEY)).toBe("light");
    saveTheme("system");
    expect(localStorage.getItem(THEME_KEY)).toBeNull();
  });

  it("does not throw when storage refuses", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(() => saveTheme("dark", broken)).not.toThrow();
    expect(warn).toHaveBeenCalledOnce();
  });
});

describe("applyTheme", () => {
  it("sets the attribute for light or dark and removes it for auto", () => {
    applyTheme("dark");
    expect(root.dataset.theme).toBe("dark");
    applyTheme("system");
    expect(root.dataset.theme).toBeUndefined();
  });
});

describe("ThemeSwitcher", () => {
  it("starts on the saved choice", () => {
    localStorage.setItem(THEME_KEY, "light");
    render(<ThemeSwitcher />);
    expect((screen.getByRole("radio", { name: "Light" }) as HTMLInputElement).checked).toBe(true);
  });

  it("changes the page and remembers the choice", async () => {
    render(<ThemeSwitcher />);
    expect((screen.getByRole("radio", { name: "Auto" }) as HTMLInputElement).checked).toBe(true);

    await userEvent.click(screen.getByRole("radio", { name: "Dark" }));
    expect(root.dataset.theme).toBe("dark");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");

    await userEvent.click(screen.getByRole("radio", { name: "Auto" }));
    expect(root.dataset.theme).toBeUndefined();
    expect(localStorage.getItem(THEME_KEY)).toBeNull();
  });

  it("follows a change made in another tab", () => {
    render(<ThemeSwitcher />);
    localStorage.setItem(THEME_KEY, "dark");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: THEME_KEY, newValue: "dark" }));
    });
    expect((screen.getByRole("radio", { name: "Dark" }) as HTMLInputElement).checked).toBe(true);
    expect(root.dataset.theme).toBe("dark");
  });
});
