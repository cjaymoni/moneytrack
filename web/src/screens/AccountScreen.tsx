import { useState } from "react";
import { Link, useParams, useSearchParams } from "react-router";
import type { Transaction } from "../api/types";
import { ApiError } from "../api/client";
import { useCategories, useInboxCount, useLedger, useRestoreTransaction } from "../api/queries";
import { KIND_NAMES, balance, isNegative, plural } from "../ui/format";
import { Failure, FormError, Loading } from "../ui/states";
import { usePageTitle } from "../ui/title";
import { NO_FILTER, type LedgerFilter, type Show } from "../ui/ledgerFilter";
import { EntryForm } from "./EntryForm";
import { CountIt } from "./CountIt";
import { LedgerList } from "./LedgerList";

/** The ledger's "show" switch: everything, rows to sort, or rows still to come. */
const SHOW_CHOICES: [Show, string][] = [
  ["all", "All"],
  ["uncategorised", "No category"],
  ["planned", "Planned"],
];

/** One account: what it holds, a line to record a row, and the ledger through today. */
export function AccountScreen() {
  const { accountId = "" } = useParams();
  const ledger = useLedger(accountId);
  const categories = useCategories();
  const restore = useRestoreTransaction();
  const inboxCount = useInboxCount();
  usePageTitle(ledger.data?.account.name ?? "Account");
  // The last row deleted here, held so it can be brought back. Cleared by undoing it or by
  // leaving the account: an undo for a row on a screen you are no longer looking at is a
  // surprise, not a convenience.
  const [deleted, setDeleted] = useState<Transaction | null>(null);
  // Whether the add form is open: `null` until the person chooses, which means open only on
  // an empty account or when they came here to add one (`?add=1`, from Home).
  const [params] = useSearchParams();
  const [adding, setAdding] = useState<boolean | null>(params.get("add") === "1" ? true : null);
  const [filter, setFilter] = useState<LedgerFilter>(NO_FILTER);
  const [heldFor, setHeldFor] = useState(accountId);
  if (heldFor !== accountId) {
    setHeldFor(accountId);
    setDeleted(null);
    setFilter(NO_FILTER);
    setAdding(params.get("add") === "1" ? true : null);
  }

  if (ledger.isPending) return <Loading />;
  if (ledger.isError) {
    if (ledger.error instanceof ApiError && ledger.error.code === "not_found") {
      return <Failure error={new ApiError(404, "not_found", "There is no such account.")} />;
    }
    return <Failure error={ledger.error} retry={() => void ledger.refetch()} />;
  }

  const { account, entries } = ledger.data;
  const formOpen = adding ?? entries.length === 0;
  const waitingHere =
    inboxCount.data?.by_account.find((each) => each.account_id === account.id)?.waiting ?? 0;

  return (
    <section className="screen">
      <header className="balance">
        <h1 className="mt-label balance__name">
          {account.name} <span className="balance__kind">· {KIND_NAMES[account.kind]}</span>
        </h1>
        <p className="balance__fig">
          <span className={isNegative(account.confirmed) ? "short" : undefined}>
            {balance(account.confirmed)}
          </span>
          <span className="balance__cur">{account.currency}</span>
        </p>
        {/* What it holds once every expected row lands is a year away now that series write a
            year ahead, which is not a number anyone plans with. The outlook is where the
            road ahead is read, with its low point and the row that runs it short. */}
        <p className="balance__meta-plain">
          Balance now. <Link to="/outlook" className="balance__link">See where it's heading</Link>
        </p>
        <CountIt key={account.id} account={account} />
      </header>

      <div className="account__actions">
        {!formOpen && (
          <button type="button" className="btn btn--primary" onClick={() => setAdding(true)}>
            Add a transaction
          </button>
        )}
      </div>

      <div className="live" role="status">
        {waitingHere > 0 && (
          <p className="review-strip">
            <span>
              {waitingHere}{" "}
              {plural(waitingHere, {
                one: "imported transaction for this account needs a quick check.",
                other: "imported transactions for this account need a quick check.",
              })}
            </span>
            <Link to="/inbox">Review now</Link>
          </p>
        )}
      </div>

      {formOpen && (
        <section className="account__add" aria-labelledby="add-heading">
          <div className="account__add-head">
            <h2 id="add-heading" className="home__heading">
              Add a transaction
            </h2>
            {entries.length > 0 && (
              <button type="button" className="btn btn--quiet" onClick={() => setAdding(false)}>
                Done
              </button>
            )}
          </div>
          <EntryForm account={account} />
        </section>
      )}

      {deleted && (
        <div className="undo" role="status">
          <p>
            Deleted <b>{deleted.label}</b>.
          </p>
          <button
            type="button"
            className="btn btn--ghost"
            disabled={restore.isPending}
            onClick={() => restore.mutate(deleted.id, { onSuccess: () => setDeleted(null) })}
          >
            {restore.isPending ? "Bringing it back" : "Undo"}
          </button>
          <button
            type="button"
            className="btn btn--quiet"
            onClick={() => setDeleted(null)}
            aria-label="Dismiss"
          >
            ×
          </button>
          <FormError error={restore.error} />
        </div>
      )}

      {entries.length > 0 && (
        <div className="ledger-filter" role="search" aria-label="Find transactions">
          <label className="field ledger-filter__text">
            <span className="mt-label">Search</span>
            <input
              type="search"
              placeholder="Description, payee or category"
              value={filter.text}
              onChange={(event) => setFilter({ ...filter, text: event.target.value })}
            />
          </label>
          <div className="segments segments--compact" role="radiogroup" aria-label="Show">
            {SHOW_CHOICES.map(([value, label]) => (
              <label key={value} className="segment">
                <input
                  type="radio"
                  name="ledger-show"
                  checked={filter.show === value}
                  onChange={() => setFilter({ ...filter, show: value })}
                />
                <span>{label}</span>
              </label>
            ))}
          </div>
        </div>
      )}

      <LedgerList
        filter={filter}
        onDeleted={setDeleted}
        entries={entries}
        opening={ledger.data.opening}
        currency={account.currency}
        categories={categories.data ?? []}
      />
    </section>
  );
}
