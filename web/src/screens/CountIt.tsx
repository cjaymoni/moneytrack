import { useState, type FormEvent } from "react";
import type { Account } from "../api/types";
import { useDeleteTransaction, useReconcile } from "../api/queries";
import { balance, signed } from "../ui/format";
import { FormError } from "../ui/states";

/**
 * "I counted my wallet, it's 340." Makes the ledger agree with a count, with one row.
 *
 * What the ledger said, the difference and the row are all the server's; this sends the
 * count as typed and says what happened, with a way to take it back.
 */
export function CountIt({ account }: { account: Account }) {
  const reconcile = useReconcile(account.id);
  const undo = useDeleteTransaction();
  const [open, setOpen] = useState(false);
  const [counted, setCounted] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    undo.reset();
    reconcile.mutate(counted.trim(), { onSuccess: () => setCounted("") });
  }

  const result = reconcile.data;
  const undone = undo.isSuccess;

  if (!open) {
    return (
      <button type="button" className="btn btn--ghost count__open" onClick={() => setOpen(true)}>
        Count it
      </button>
    );
  }

  return (
    <div className="count">
      <form className="count__form" onSubmit={submit} aria-label={`Count ${account.name}`}>
        <label className="field">
          <span className="mt-label">What it actually holds now, {account.currency}</span>
          <input
            className="amount-input"
            inputMode="decimal"
            autoComplete="off"
            required
            placeholder={account.confirmed.display}
            value={counted}
            onChange={(event) => setCounted(event.target.value)}
          />
          <span className="field__help">
            From the wallet, the MoMo balance or the bank. If it differs from the ledger, one
            row is recorded to make them agree.
          </span>
        </label>
        <div className="count__actions">
          <button type="button" className="btn btn--quiet" onClick={() => setOpen(false)}>
            Close
          </button>
          <button type="submit" className="btn btn--primary" disabled={reconcile.isPending}>
            {reconcile.isPending ? "Checking" : "Make it agree"}
          </button>
        </div>
      </form>

      <FormError error={reconcile.error ?? undo.error} />

      {result && !result.adjustment && (
        <p className="count__result" role="status">
          It agrees: the ledger also says {balance(result.counted)} {account.currency}.
        </p>
      )}
      {result?.adjustment && !undone && (
        <div className="count__result" role="status">
          <p>
            The ledger said {balance(result.ledger)}; you counted {balance(result.counted)}.
            Recorded a correction of{" "}
            <b>{signed(result.adjustment.amount, result.adjustment.direction)}</b>.
          </p>
          <button
            type="button"
            className="btn btn--ghost"
            disabled={undo.isPending}
            onClick={() => result.adjustment && undo.mutate(result.adjustment)}
          >
            {undo.isPending ? "Taking it back" : "Undo"}
          </button>
        </div>
      )}
      {result?.adjustment && undone && (
        <p className="count__result" role="status">
          Taken back. The ledger is as it was.
        </p>
      )}
    </div>
  );
}
