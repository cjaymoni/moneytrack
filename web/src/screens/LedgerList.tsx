import { useEffect, useId, useRef, useState } from "react";
import type { Category, LedgerEntry, Money } from "../api/types";
import { useSetCategory } from "../api/queries";
import { CategoryPicker, fullName, kindFor } from "../ui/CategoryPicker";
import { balance, gutterDate, isNegative, longDate, signed } from "../ui/format";
import { FormError } from "../ui/states";

/**
 * The ledger: one list, oldest first, running through a ruled TODAY line into the rows still
 * expected.
 *
 * The running balance beside each row is `mt-core`'s walk as the server sent it. Nothing
 * here sums anything; where a figure is negative, that is read off its sign.
 */
export function LedgerList({
  entries,
  opening,
  currency,
  categories = [],
  now = new Date(),
}: {
  entries: LedgerEntry[];
  opening: Money;
  currency: string;
  categories?: Category[];
  now?: Date;
}) {
  const todayRef = useRef<HTMLDivElement>(null);

  // Which rows are history and which are forecast is a matter of when they are booked, read
  // against the reader's clock. Pending rows in the past stay where they fell, marked.
  const past = entries.filter((entry) => new Date(entry.transaction.booked_at) <= now);
  const future = entries.filter((entry) => new Date(entry.transaction.booked_at) > now);

  // Long ledgers open at today rather than at the first row ever recorded.
  useEffect(() => {
    if (past.length > 8) todayRef.current?.scrollIntoView({ block: "center" });
  }, [past.length]);

  if (entries.length === 0) {
    return (
      <div className="ledger ledger--empty">
        <p className="ledger__empty-title">Nothing recorded yet.</p>
        <p className="ledger__empty-copy">
          Opened with {balance(opening)} {currency}. The first row you record will appear
          here, with what the account held after it.
        </p>
      </div>
    );
  }

  return (
    <section className="ledger" aria-label="Ledger">
      <div className="ledger__head" aria-hidden="true">
        <span />
        <span className="mt-micro">Row</span>
        <span className="mt-micro ledger__head-num">Amount · then held</span>
      </div>
      <p className="ledger__opening">
        <span className="mt-micro">Opened with</span>
        <span className="ledger__opening-fig">{balance(opening)}</span>
      </p>
      <ol className="ledger__rows">
        {past.map((entry) => (
          <Row key={entry.transaction.id} entry={entry} future={false} categories={categories} />
        ))}
      </ol>
      <div className="mt-today" ref={todayRef} role="separator" aria-label="Today">
        <span className="mt-micro">Today</span>
        <i />
      </div>
      {future.length > 0 && (
        <ol className="ledger__rows">
          {future.map((entry) => (
            <Row key={entry.transaction.id} entry={entry} future categories={categories} />
          ))}
        </ol>
      )}
    </section>
  );
}

function Row({
  entry,
  future,
  categories,
}: {
  entry: LedgerEntry;
  future: boolean;
  categories: Category[];
}) {
  const { transaction, balance_after } = entry;
  const pending = transaction.status === "pending";
  const classes = ["mt-row", future || pending ? "mt-row--future" : ""].join(" ").trim();
  const [filing, setFiling] = useState(false);
  const setCategory = useSetCategory();
  const pickerId = useId();
  const category = categories.find((each) => each.id === transaction.category_id);

  return (
    <li className={classes}>
      <time className="date" dateTime={transaction.booked_at} title={longDate(transaction.booked_at)}>
        {gutterDate(transaction.booked_at)}
      </time>
      <div>
        <div className="label">{transaction.label}</div>
        <div className="sub">
          {transaction.counterparty && (
            <>
              <span>{transaction.counterparty}</span>
              <span aria-hidden="true"> · </span>
            </>
          )}
          <button
            type="button"
            className={category ? "filed" : "filed filed--none"}
            aria-expanded={filing}
            aria-controls={pickerId}
            onClick={() => setFiling(!filing)}
          >
            <span className="visually-hidden">Category: </span>
            {category ? fullName(category, categories) : "Uncategorised"}
          </button>
          {pending && <span aria-hidden="true"> · </span>}
          {pending && <span>expected</span>}
          <Verification source={transaction.source} pending={pending} />
        </div>
      </div>
      <div className="figures">
        <div className={transaction.direction === "in" ? "amt in" : "amt"}>
          {signed(transaction.amount, transaction.direction)}
        </div>
        <div className={isNegative(balance_after) ? "after short" : "after"}>
          <span className="visually-hidden">then held </span>
          {balance(balance_after)}
        </div>
      </div>
      {filing && (
        <div className="refile" id={pickerId}>
          <CategoryPicker
            categories={categories}
            kind={kindFor(transaction.direction)}
            value={transaction.category_id}
            disabled={setCategory.isPending}
            label={`Category for ${transaction.label}`}
            onChange={(categoryId) =>
              setCategory.mutate(
                { id: transaction.id, categoryId },
                { onSuccess: () => setFiling(false) },
              )
            }
          />
          <FormError error={setCategory.error} />
        </div>
      )}
    </li>
  );
}

/**
 * How the row got here. Every settled row says: a hand-entered one with the by-hand mark.
 * A row nobody has settled yet — an expected one — carries no mark until it happens.
 *
 * Parsed rows and their stamp arrive with import and SMS; this client has only rows a person
 * typed, and says so.
 */
function Verification({ source, pending }: { source: string; pending: boolean }) {
  if (pending) return null;
  if (source === "manual") return <span className="mt-byhand">By hand</span>;
  return null;
}
