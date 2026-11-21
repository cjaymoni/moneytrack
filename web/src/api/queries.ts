// Server state, fetched and cached per query and invalidated on mutation.
//
// There is no client-side model of the ledger. After a write, the queries that could have
// changed are thrown away and asked again, and the server's walk is the only one there is.

import { QueryClient, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError, get, post } from "./client";
import type {
  Account,
  Breakdown,
  Category,
  NewCategory,
  Currency,
  Ledger,
  Money,
  NewAccount,
  NewTransaction,
  Transaction,
  User,
} from "./types";

export const keys = {
  me: ["me"] as const,
  accounts: ["accounts"] as const,
  ledger: (accountId: string) => ["ledger", accountId] as const,
  totals: ["totals"] as const,
  currencies: ["currencies"] as const,
  categories: ["categories"] as const,
  breakdown: (month: string) => ["breakdown", month] as const,
};

/** Who is signed in, or `null` for nobody. Any other failure is an error. */
export function useMe() {
  return useQuery({
    queryKey: keys.me,
    queryFn: async (): Promise<User | null> => {
      try {
        return await get<User>("/me");
      } catch (error) {
        if (error instanceof ApiError && error.code === "unauthenticated") return null;
        throw error;
      }
    },
    staleTime: Infinity,
  });
}

export function useAccounts() {
  return useQuery({ queryKey: keys.accounts, queryFn: () => get<Account[]>("/accounts") });
}

export function useLedger(accountId: string) {
  return useQuery({
    queryKey: keys.ledger(accountId),
    queryFn: () => get<Ledger>(`/ledger?account_id=${encodeURIComponent(accountId)}`),
  });
}

/** What is held in each currency, never converted into one number. */
export function useTotals() {
  return useQuery({
    queryKey: keys.totals,
    queryFn: () => get<Money[]>("/totals?basis=confirmed"),
  });
}

export function useCurrencies() {
  return useQuery({
    queryKey: keys.currencies,
    queryFn: () => get<Currency[]>("/currencies"),
    staleTime: Infinity,
  });
}

/**
 * Where the money went in a month, one breakdown per currency held.
 *
 * The reader's offset goes with it, so a row near midnight lands in the month on their own
 * clock. Grouping, totals and shares are all `mt-core`'s.
 */
export function useBreakdown(month: string) {
  return useQuery({
    queryKey: keys.breakdown(month),
    queryFn: () => {
      const offset = -new Date().getTimezoneOffset();
      return get<Breakdown[]>(
        `/breakdown?month=${encodeURIComponent(month)}&offset_minutes=${offset}`,
      );
    },
  });
}

export function useCategories() {
  return useQuery({ queryKey: keys.categories, queryFn: () => get<Category[]>("/categories") });
}

export function useCreateCategory() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (category: NewCategory) => post<Category>("/categories", category),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.categories });
      void client.invalidateQueries({ queryKey: ["breakdown"] });
    },
  });
}

/**
 * Files a row under a category, or under none. Moves no balance, so the accounts and totals
 * stand; the ledger and the breakdowns, which name the category, are asked again.
 */
export function useSetCategory() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, categoryId }: { id: string; categoryId: string | null }) =>
      post<Transaction>(`/transactions/${encodeURIComponent(id)}/category`, {
        category_id: categoryId,
      }),
    onSuccess: (row) => {
      void client.invalidateQueries({ queryKey: keys.ledger(row.account_id) });
      void client.invalidateQueries({ queryKey: ["breakdown"] });
    },
  });
}

/**
 * Drops every cached answer except who is signed in.
 *
 * Not `clear()`: that also drops the `me` query the app is watching, and setting it again
 * afterwards creates a new query nobody is subscribed to — the sign-in succeeds and the
 * screen never moves.
 */
function forgetLedger(client: QueryClient) {
  client.removeQueries({ predicate: (query) => query.queryKey[0] !== keys.me[0] });
}

interface Credentials {
  email: string;
  password: string;
}

/** Signs in or registers, then starts from a clean cache: nothing of a previous person's. */
export function useSignIn(mode: "login" | "register") {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (credentials: Credentials) => post<User>(`/auth/${mode}`, credentials),
    onSuccess: (user) => {
      forgetLedger(client);
      client.setQueryData(keys.me, user);
    },
  });
}

export function useSignOut() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => post<unknown>("/auth/logout", {}),
    onSettled: () => {
      // Whatever the server said, this page forgets everything it was shown.
      forgetLedger(client);
      client.setQueryData(keys.me, null);
    },
  });
}

export function useCreateAccount() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (account: NewAccount) => post<Account>("/accounts", account),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.accounts });
      void client.invalidateQueries({ queryKey: keys.totals });
    },
  });
}

export function useCreateTransaction() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (row: NewTransaction) => post<Transaction>("/transactions", row),
    onSuccess: (row) => {
      void client.invalidateQueries({ queryKey: keys.ledger(row.account_id) });
      void client.invalidateQueries({ queryKey: keys.accounts });
      void client.invalidateQueries({ queryKey: keys.totals });
      void client.invalidateQueries({ queryKey: ["breakdown"] });
    },
  });
}
