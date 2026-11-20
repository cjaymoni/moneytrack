import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router";

// Archivo is served from this origin, not from a font CDN: a finance app's pages should not
// tell a third party who opened them and when.
import "@fontsource/archivo/400.css";
import "@fontsource/archivo/500.css";
import "@fontsource/archivo/600.css";
import "@fontsource/archivo/700.css";
import "./styles.css";

import { App } from "./App";
import { ApiError } from "./api/client";

const queries = new QueryClient({
  defaultOptions: {
    queries: {
      // A refusal will not change on a retry; a dropped connection might.
      retry: (failures, error) =>
        failures < 2 && error instanceof ApiError && (error.status === 0 || error.status >= 500),
      refetchOnWindowFocus: true,
    },
  },
});

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <QueryClientProvider client={queries}>
        <BrowserRouter>
          <App />
        </BrowserRouter>
      </QueryClientProvider>
    </StrictMode>,
  );
}
