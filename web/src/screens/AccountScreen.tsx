import { useState } from "react";
import { useParams } from "react-router";
import type { Transaction } from "../api/types";
import { ApiError } from "../api/client";
import { useCategories, useLedger, useRestoreTransaction } from "../api/queries";
import { KIND_NAMES, balance, isNegative } from "../ui/format";
import { Failure, FormError, Loading } from "../ui/states";
import { EntryForm } from "./EntryForm";
import { CountIt } from "./CountIt";
import { LedgerList } from "./LedgerList";

/** One account: what it holds, a line to record a row, and the ledger through today. */
export function AccountScreen() {
  const { accountId = "" } = useParams();
  const ledger = useLedger(accountId);
  const categories = useCategories();
  const restore = useRestoreTransaction();
  // The last row deleted here, held so it can be brought back. Cleared by undoing it or by
  // leaving the account: an undo for a row on a screen you are no longer looking at is a
  // surprise, not a convenience.
  const [deleted, setDeleted] = useState<Transaction | null>(null);
  const [heldFor, setHeldFor] = useState(accountId);
  if (heldFor !== accountId) {
    setHeldFor(accountId);
    setDeleted(null);
  }

  if (ledger.isPending) return <Loading />;
  if (ledger.isError) {
    if (ledger.error instanceof ApiError && ledger.error.code === "not_found") {
      return <Failure error={new ApiError(404, "not_found", "There is no such account.")} />;
    }
    return <Failure error={ledger.error} retry={() => void ledger.refetch()} />;
  }

  const { account, entries } = ledger.data;
  const forecastDiffers = account.projected.minor_units !== account.confirmed.minor_units;

  return (
    <section className="screen">
      <header className="balance">
        <p className="mt-label balance__name">
          {account.name} <span className="balance__kind">· {KIND_NAMES[account.kind]}</span>
        </p>
        <p className="balance__fig">
          <span className={isNegative(account.confirmed) ? "short" : undefined}>
            {balance(account.confirmed)}
          </span>
          <span className="balance__cur">{account.currency}</span>
        </p>
        <p className="mt-label balance__meta">
          {forecastDiffers ? (
            <>
              Held now · <span className="balance__then">{balance(account.projected)}</span> once
              expected rows land
            </>
          ) : (
            "Held now"
          )}
        </p>
        <CountIt key={account.id} account={account} />
      </header>

      <EntryForm account={account} />

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

      <LedgerList
        onDeleted={setDeleted}
        entries={entries}
        opening={ledger.data.opening}
        currency={account.currency}
        categories={categories.data ?? []}
      />
    </section>
  );
}
