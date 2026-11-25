// Server state, fetched and cached per query and invalidated on mutation.
//
// There is no client-side model of the ledger. After a write, the queries that could have
// changed are thrown away and asked again, and the server's walk is the only one there is.

import {
  QueryClient,
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { ApiError, del, get, patch, post } from "./client";
import type {
  Account,
  Breakdown,
  Budget,
  CountResult,
  Daily,
  CsvMapping,
  ImportFormat,
  ImportOutcome,
  ImportPreview,
  Inbox,
  InboxCount,
  NewBudget,
  NewRecurring,
  Recurring,
  Outlook,
  Category,
  NewCategory,
  Currency,
  Ledger,
  Money,
  NewAccount,
  NewTransaction,
  Transaction,
  TransactionEdit,
  Trend,
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
  trend: (until: string, months: number) => ["trend", until, months] as const,
  daily: (month: string) => ["daily", month] as const,
  outlook: (days: number) => ["outlook", days] as const,
  budgets: ["budgets"] as const,
  recurring: ["recurring"] as const,
  inbox: ["inbox"] as const,
  transactions: ["transactions"] as const,
  // Under the inbox's key, so anything that invalidates the inbox invalidates this too.
  inboxCount: ["inbox", "count"] as const,
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

/**
 * Money in and out for the `months` months ending with `until`, one trend per currency held.
 *
 * Every figure, the scale and every bar's height are `mt-core`'s. While a new range loads,
 * the last one stays on screen rather than the chart blinking out.
 */
export function useTrend(until: string, months: number) {
  return useQuery({
    queryKey: keys.trend(until, months),
    queryFn: () =>
      get<Trend[]>(
        `/trend?until=${encodeURIComponent(until)}&months=${months}&offset_minutes=${offsetMinutes()}`,
      ),
    placeholderData: keepPreviousData,
  });
}

/** A month's spending day by day, one per currency held. Heights and scale are `mt-core`'s. */
export function useDaily(month: string) {
  return useQuery({
    queryKey: keys.daily(month),
    queryFn: () =>
      get<Daily[]>(`/daily?month=${encodeURIComponent(month)}&offset_minutes=${offsetMinutes()}`),
    placeholderData: keepPreviousData,
  });
}

/** Where each account is headed over the next `days`, and the row that runs it short. */
export function useOutlook(days: number) {
  return useQuery({
    queryKey: keys.outlook(days),
    queryFn: () => get<Outlook[]>(`/outlook?days=${days}`),
  });
}

/** The reader's offset from UTC in minutes, east positive — which day "today" is. */
const offsetMinutes = () => -new Date().getTimezoneOffset();

/** Every budget, with where it stands today on the reader's clock. */
export function useBudgets() {
  return useQuery({
    queryKey: keys.budgets,
    queryFn: () => get<Budget[]>(`/budgets?offset_minutes=${offsetMinutes()}`),
  });
}

export function useCreateBudget() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (budget: NewBudget) =>
      post<Budget>("/budgets", { ...budget, offset_minutes: offsetMinutes() }),
    onSuccess: () => void client.invalidateQueries({ queryKey: keys.budgets }),
  });
}

/** Changes a cap from the current period on; past periods keep theirs. */
export function useChangeBudget() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, cap, warn_at_bps }: { id: string; cap: string; warn_at_bps: number }) =>
      patch<Budget>(`/budgets/${encodeURIComponent(id)}`, {
        cap,
        warn_at_bps,
        offset_minutes: offsetMinutes(),
      }),
    onSuccess: () => void client.invalidateQueries({ queryKey: keys.budgets }),
  });
}

export function useDeleteBudget() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => del<unknown>(`/budgets/${encodeURIComponent(id)}`),
    onSuccess: () => void client.invalidateQueries({ queryKey: keys.budgets }),
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
      void client.invalidateQueries({ queryKey: ["trend"] });
      void client.invalidateQueries({ queryKey: ["daily"] });
    },
  });
}

/**
 * Files a row under a category, or under none. Moves no balance, so the accounts and totals
 * stand; the ledger, the breakdowns and the budgets, which all group by category, are asked
 * again.
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
      void client.invalidateQueries({ queryKey: keys.transactions });
      void client.invalidateQueries({ queryKey: ["breakdown"] });
      void client.invalidateQueries({ queryKey: ["trend"] });
      void client.invalidateQueries({ queryKey: ["daily"] });
      void client.invalidateQueries({ queryKey: keys.budgets });
    },
  });
}

/** Every row in every account, oldest first: what the uncategorised view sorts through. */
export function useAllTransactions() {
  return useQuery({ queryKey: keys.transactions, queryFn: () => get<Transaction[]>("/transactions") });
}

/** What filing several rows at once sends: the rows, where they go, and a payee to remember. */
export interface FileMany {
  transactionIds: string[];
  categoryId: string | null;
  remember?: { matched_field: "counterparty" | "label"; pattern: string };
}

/**
 * Files several rows under one category in one request, and remembers the payee for the
 * next statement if asked. Every view that groups by category is asked again.
 */
export function useFileMany() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ transactionIds, categoryId, remember }: FileMany) =>
      post<{ filed: number }>("/transactions/category", {
        transaction_ids: transactionIds,
        category_id: categoryId,
        ...(remember ? { remember } : {}),
      }),
    onSuccess: () => afterCategoryChange(client),
  });
}

export function useRenameCategory() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) =>
      patch<Category>(`/categories/${encodeURIComponent(id)}`, { name }),
    onSuccess: () => afterCategoryChange(client),
  });
}

/** Deletes a category, moving its rows to `moveTo` or to no category. */
export function useDeleteCategory() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, moveTo }: { id: string; moveTo: string | null }) =>
      del<unknown>(
        `/categories/${encodeURIComponent(id)}${moveTo ? `?move_to=${encodeURIComponent(moveTo)}` : ""}`,
      ),
    onSuccess: () => afterCategoryChange(client),
  });
}

/** Every query that groups by category or shows a row's category. */
function afterCategoryChange(client: QueryClient) {
  for (const key of [keys.categories, keys.transactions, keys.budgets, keys.recurring, keys.inbox]) {
    void client.invalidateQueries({ queryKey: key });
  }
  for (const prefix of ["ledger", "breakdown", "trend", "daily"]) {
    void client.invalidateQueries({ queryKey: [prefix] });
  }
}

/** Every query a change to one row in one account can have changed. */
function afterRowChange(client: QueryClient, accountId: string) {
  void client.invalidateQueries({ queryKey: keys.budgets });
  void client.invalidateQueries({ queryKey: keys.transactions });
  void client.invalidateQueries({ queryKey: ["outlook"] });
  void client.invalidateQueries({ queryKey: keys.ledger(accountId) });
  void client.invalidateQueries({ queryKey: keys.accounts });
  void client.invalidateQueries({ queryKey: keys.totals });
  void client.invalidateQueries({ queryKey: ["breakdown"] });
  void client.invalidateQueries({ queryKey: ["trend"] });
  void client.invalidateQueries({ queryKey: ["daily"] });
}

/**
 * Makes an account agree with a count. The difference, and the row that closes it, are
 * `mt-core`'s; this sends what was counted, as typed.
 */
export function useReconcile(accountId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (counted: string) =>
      post<CountResult>(`/accounts/${encodeURIComponent(accountId)}/reconcile`, { counted }),
    onSuccess: (result) => {
      if (result.adjustment) afterRowChange(client, accountId);
    },
  });
}

export function useRecurring() {
  return useQuery({ queryKey: keys.recurring, queryFn: () => get<Recurring[]>("/recurring") });
}

/** Everything a series starting or stopping can change: it writes or removes expected rows. */
function afterSeriesChange(client: QueryClient) {
  void client.invalidateQueries({ queryKey: keys.recurring });
  void client.invalidateQueries({ queryKey: ["ledger"] });
  void client.invalidateQueries({ queryKey: keys.accounts });
  void client.invalidateQueries({ queryKey: keys.totals });
  void client.invalidateQueries({ queryKey: ["outlook"] });
  void client.invalidateQueries({ queryKey: ["breakdown"] });
  void client.invalidateQueries({ queryKey: ["trend"] });
  void client.invalidateQueries({ queryKey: ["daily"] });
  void client.invalidateQueries({ queryKey: keys.budgets });
}

export function useStartRecurring() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (series: NewRecurring) =>
      post<Recurring>("/recurring", { ...series, offset_minutes: offsetMinutes() }),
    onSuccess: () => afterSeriesChange(client),
  });
}

/** Stops a series. Its expected rows still to come go with it; what has happened stays. */
export function useStopRecurring() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => del<unknown>(`/recurring/${encodeURIComponent(id)}`),
    onSuccess: () => afterSeriesChange(client),
  });
}

/** Changes a row, sent whole. */
export function useEditTransaction() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, edit }: { id: string; edit: TransactionEdit }) =>
      patch<Transaction>(`/transactions/${encodeURIComponent(id)}`, edit),
    onSuccess: (row) => afterRowChange(client, row.account_id),
  });
}

/** Deletes a row. The server keeps a tombstone, so it can be undone. */
export function useDeleteTransaction() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (row: Transaction) =>
      del<unknown>(`/transactions/${encodeURIComponent(row.id)}`).then(() => row),
    onSuccess: (row) => afterRowChange(client, row.account_id),
  });
}

/** Undoes a deletion, bringing the row back exactly as it was. */
export function useRestoreTransaction() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      post<Transaction>(`/transactions/${encodeURIComponent(id)}/restore`, {}),
    onSuccess: (row) => afterRowChange(client, row.account_id),
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
      void client.invalidateQueries({ queryKey: ["outlook"] });
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
      void client.invalidateQueries({ queryKey: ["trend"] });
      void client.invalidateQueries({ queryKey: ["daily"] });
      void client.invalidateQueries({ queryKey: ["outlook"] });
      void client.invalidateQueries({ queryKey: keys.budgets });
    },
  });
}

// ---------------------------------------------------------------------------- imports ----

/** The lines waiting for review, with what each might duplicate. */
export function useInbox() {
  return useQuery({ queryKey: keys.inbox, queryFn: () => get<Inbox>("/inbox") });
}

/** How many lines wait, in all and per account. Cheap: nothing is read or matched. */
export function useInboxCount() {
  return useQuery({ queryKey: keys.inboxCount, queryFn: () => get<InboxCount>("/inbox/count") });
}

/** Splits the start of a CSV file into cells, and fetches the account's last mapping. */
export function usePreviewImport() {
  return useMutation({
    mutationFn: (request: { account_id: string; content_base64: string }) =>
      post<ImportPreview>("/imports/preview", request),
  });
}

export interface ImportRequest {
  account_id: string;
  file_name: string;
  format: ImportFormat;
  content_base64: string;
  mapping?: CsvMapping;
}

/** Uploads a statement. Its lines go to the inbox, never straight to the ledger. */
export function useImport() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (request: ImportRequest) =>
      post<ImportOutcome>("/imports", { ...request, offset_minutes: offsetMinutes() }),
    onSuccess: () => void client.invalidateQueries({ queryKey: keys.inbox }),
  });
}

/**
 * Everything settling an inbox line can change. Accepting writes rows, merging can confirm
 * an expected one, and a learned rule changes the next proposal; which account it touched
 * is not worth tracking for a screen the person is about to leave anyway.
 */
function afterSettling(client: QueryClient) {
  void client.invalidateQueries({ queryKey: keys.inbox });
  void client.invalidateQueries({ queryKey: ["ledger"] });
  void client.invalidateQueries({ queryKey: keys.accounts });
  void client.invalidateQueries({ queryKey: keys.totals });
  void client.invalidateQueries({ queryKey: ["outlook"] });
  void client.invalidateQueries({ queryKey: ["breakdown"] });
  void client.invalidateQueries({ queryKey: ["trend"] });
  void client.invalidateQueries({ queryKey: ["daily"] });
  void client.invalidateQueries({ queryKey: keys.budgets });
}

/** Accepts lines into the ledger, each filed where the person chose. */
export function useAcceptInbox() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (items: { id: string; category_id: string | null }[]) =>
      post<{ accepted: number }>("/inbox/accept", { items }),
    onSuccess: () => afterSettling(client),
  });
}

/** Settles a line as a row already in the ledger. */
export function useMergeInbox() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, transactionId }: { id: string; transactionId: string }) =>
      post<unknown>(`/inbox/${encodeURIComponent(id)}/merge`, { transaction_id: transactionId }),
    onSuccess: () => afterSettling(client),
  });
}

/** Settles a line as not a movement worth recording. */
export function useRejectInbox() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => post<unknown>(`/inbox/${encodeURIComponent(id)}/reject`, {}),
    onSuccess: () => afterSettling(client),
  });
}
