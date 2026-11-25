import { useEffect, useState } from "react";

/**
 * Light, dark, or whatever the device is set to.
 *
 * The tokens do the work: `design/tokens/tokens.css` follows the device unless the root
 * carries `data-theme`, so "system" is the attribute's absence, not a third palette. The
 * choice is a convenience kept in this browser only — it is not part of the account and
 * never goes to the server.
 */
export type ThemeChoice = "system" | "light" | "dark";

export const THEME_CHOICES: { value: ThemeChoice; name: string }[] = [
  { value: "system", name: "Auto" },
  { value: "light", name: "Light" },
  { value: "dark", name: "Dark" },
];

/** The storage key. `index.html` reads the same key before first paint; keep the two alike. */
export const THEME_KEY = "mt-theme";

function isChoice(value: unknown): value is ThemeChoice {
  return value === "system" || value === "light" || value === "dark";
}

/**
 * The saved choice, or "system" when there is none or it cannot be read.
 *
 * Storage can throw — a private window, blocked site data — and a theme is not worth failing
 * a page over, so a failure is reported to the console and the device's setting is used.
 */
export function readTheme(storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage): ThemeChoice {
  try {
    const saved = storage?.getItem(THEME_KEY);
    return isChoice(saved) ? saved : "system";
  } catch (error) {
    console.warn("theme: could not read the saved choice; following the device", error);
    return "system";
  }
}

/** Remembers the choice in this browser. Not remembering it leaves the page working. */
export function saveTheme(
  choice: ThemeChoice,
  storage: Pick<Storage, "setItem" | "removeItem"> | undefined = globalThis.localStorage,
): void {
  try {
    if (choice === "system") storage?.removeItem(THEME_KEY);
    else storage?.setItem(THEME_KEY, choice);
  } catch (error) {
    console.warn("theme: could not save the choice; it lasts until the page is closed", error);
  }
}

/** Puts the choice on the page: an attribute for light or dark, none for the device's own. */
export function applyTheme(choice: ThemeChoice, root: HTMLElement = document.documentElement): void {
  if (choice === "system") delete root.dataset.theme;
  else root.dataset.theme = choice;
}

/**
 * The current choice and a way to change it. A change in another tab of the app follows
 * here too, so two open tabs never disagree.
 */
export function useTheme(): [ThemeChoice, (choice: ThemeChoice) => void] {
  const [choice, setChoice] = useState<ThemeChoice>(() => readTheme());

  useEffect(() => {
    const follow = (event: StorageEvent) => {
      if (event.key !== THEME_KEY && event.key !== null) return;
      const next = readTheme();
      applyTheme(next);
      setChoice(next);
    };
    window.addEventListener("storage", follow);
    return () => window.removeEventListener("storage", follow);
  }, []);

  const choose = (next: ThemeChoice) => {
    applyTheme(next);
    saveTheme(next);
    setChoice(next);
  };
  return [choice, choose];
}
