import type { ReactElement } from "react";
import { render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";

/** Renders inside a query client of its own, with no retries to wait out. */
export function renderWithClient(ui: ReactElement) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}

/** As `renderWithClient`, inside a router, for components that link somewhere. */
export function renderInApp(ui: ReactElement) {
  return renderWithClient(<MemoryRouter>{ui}</MemoryRouter>);
}
