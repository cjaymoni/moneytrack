import { useEffect } from "react";

/** The app's name, after the page's own in every tab title. */
export const APP_NAME = "moneytrack";

/** "Home · moneytrack": the page first, so a row of tabs can be told apart. */
export function pageTitle(page: string | undefined): string {
  const trimmed = page?.trim();
  return trimmed ? `${trimmed} · ${APP_NAME}` : APP_NAME;
}

/**
 * Names the browser tab after the page, so a person — or a screen reader reading the title
 * after a page change — knows where they are. Leaving the page puts the app's name back.
 */
export function usePageTitle(page: string | undefined): void {
  useEffect(() => {
    document.title = pageTitle(page);
    return () => {
      document.title = APP_NAME;
    };
  }, [page]);
}
