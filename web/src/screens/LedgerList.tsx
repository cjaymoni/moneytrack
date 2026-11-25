import { useId, useRef, useState } from "react";
import { Link } from "react-router";
import type { Category, LedgerEntry, Money, Transaction } from "../api/types";
import { NO_FILTER, isFiltering, matches, type LedgerFilter } from "../ui/ledgerFilter";

/** How far ahead the ledger lists expected rows. */
const AHEAD_DAYS = 45;
/** How many recent transactions show before "Show earlier". */
const RECENT_SHOWN = 20;

import { useEditTransaction, useSetCategory } from "../api/queries";
import { CategoryPicker, fullName, kindFor } from "../ui/CategoryPicker";
import { balance, gutterDate, isNegative, longDate, plural, signed } from "../ui/format";
import { FormError } from "../ui/states";
import { RowEditor } from "./RowEditor";

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
  onDeleted = () => {},
  filter = NO_FILTER,
}: {
  entries: LedgerEntry[];
  opening: Money;
  currency: string;
  categories?: Category[];
  now?: Date;
  /** Told when a row is deleted, so the screen can offer to undo it. */
  onDeleted?: (row: Transaction) => void;
  /** What the person is searching for; the whole ledger when absent. */
  filter?: LedgerFilter;
}) {
  const todayRef = useRef<HTMLDivElement>(null);
  const [showAll, setShowAll] = useState(false);
  const filtering = isFiltering(filter);
  const nameOf = (id: string | null) => categories.find((category) => category.id === id)?.name;
  const shownEntries = filtering
    ? entries.filter((entry) => matches(entry, filter, nameOf(entry.transaction.category_id) ?? undefined))
    : entries;

  // Which rows are history and which are forecast is a matter of when they are booked, read
  // against the reader's clock. Pending rows in the past stay where they fell, marked.
  const allPast = shownEntries.filter((entry) => new Date(entry.transaction.booked_at) <= now);
  // A long history opens on its most recent rows; the rest are a click away. A search shows
  // every match.
  const earlier = filtering || showAll ? 0 : Math.max(0, allPast.length - RECENT_SHOWN);
  const past = allPast.slice(earlier);
  // The forecast is shown for the weeks ahead, not the year a repeating series writes: past
  // that, a list stops being something a person reads, and the outlook reads it for them.
  const horizon = new Date(now.getTime() + AHEAD_DAYS * 86_400_000);
  const ahead = shownEntries.filter((entry) => new Date(entry.transaction.booked_at) > now);
  const future = ahead.filter((entry) => new Date(entry.transaction.booked_at) <= horizon);
  const beyond = ahead.length - future.length;

  // A long ledger already opens on its recent rows (see `earlier`), so the page is not
  // scrolled for the person: scrolling would carry them past the add form above.

  if (entries.length === 0) {
    return (
      <div className="ledger ledger--empty">
        <p className="ledger__empty-title">No transactions yet.</p>
        <p className="ledger__empty-copy">
          Starting balance {balance(opening)} {currency}. Transactions you add will show here,
          each with the balance after it.
        </p>
      </div>
    );
  }

  if (filtering && shownEntries.length === 0) {
    return (
      <p className="ledger ledger--empty field__help" role="status">
        No transactions match. Try fewer words, or show all transactions.
      </p>
    );
  }

  return (
    <section className="ledger" aria-label="Transactions">
      <div className="ledger__head" aria-hidden="true">
        <span />
        <span className="mt-micro">Transaction</span>
        <span className="mt-micro ledger__head-num">Amount · balance after</span>
      </div>
      {earlier > 0 ? (
        <button type="button" className="btn btn--quiet ledger__earlier" onClick={() => setShowAll(true)}>
          Show {earlier} earlier {plural(earlier, { one: "transaction", other: "transactions" })}
        </button>
      ) : (
        !filtering && (
          <p className="ledger__opening">
            <span className="mt-micro">Starting balance</span>
            <span className="ledger__opening-fig">{balance(opening)}</span>
          </p>
        )
      )}
      <ol className="ledger__rows">
        {past.map((entry) => (
          <Row
            key={entry.transaction.id}
            entry={entry}
            future={false}
            categories={categories}
            onDeleted={onDeleted}
            due
          />
        ))}
      </ol>
      <div className="mt-today" ref={todayRef} role="separator" aria-label="Today">
        <span className="mt-micro">Today</span>
        <i />
      </div>
      {future.length > 0 && (
        <ol className="ledger__rows">
          {future.map((entry) => (
            <Row
              key={entry.transaction.id}
              entry={entry}
              future
              categories={categories}
              onDeleted={onDeleted}
            />
          ))}
        </ol>
      )}
      {beyond > 0 && (
        <p className="ledger__beyond field__help">
          {beyond} more planned {beyond === 1 ? "transaction" : "transactions"} after the next {AHEAD_DAYS} days.{" "}
          <Link to="/outlook?days=180">See them in the forecast</Link>
        </p>
      )}
    </section>
  );
}

function Row({
  entry,
  future,
  categories,
  onDeleted,
  due = false,
}: {
  entry: LedgerEntry;
  future: boolean;
  categories: Category[];
  onDeleted: (row: Transaction) => void;
  /** Dated now or earlier, so an expected row here is one that should have happened. */
  due?: boolean;
}) {
  const { transaction, balance_after } = entry;
  const pending = transaction.status === "pending";
  const classes = ["mt-row", future || pending ? "mt-row--future" : ""].join(" ").trim();
  // One thing open under a row at a time: the quick refile, or the whole editor.
  const [open, setOpen] = useState<"none" | "file" | "edit">("none");
  const filing = open === "file";
  const editing = open === "edit";
  const setCategory = useSetCategory();
  const confirm = useEditTransaction();
  const pickerId = useId();
  const editorId = useId();
  const category = categories.find((each) => each.id === transaction.category_id);

  return (
    <li className={classes}>
      <time className="date" dateTime={transaction.booked_at} title={longDate(transaction.booked_at)}>
        {gutterDate(transaction.booked_at)}
      </time>
      <div>
        <button
          type="button"
          className="label label--edit"
          aria-label={`${transaction.label}, edit`}
          aria-expanded={editing}
          aria-controls={editorId}
          onClick={() => setOpen(editing ? "none" : "edit")}
        >
          {transaction.label}
          <span className="label__edit" aria-hidden="true">
            Edit
          </span>
        </button>
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
            aria-label={category ? `Category: ${fullName(category, categories)}` : "No category. Add category"}
            aria-expanded={filing}
            aria-controls={pickerId}
            onClick={() => setOpen(filing ? "none" : "file")}
          >
            {category ? fullName(category, categories) : "+ Add category"}
          </button>
          {transaction.recurring_id && (
            <>
              <span aria-hidden="true"> · </span>
              <span>repeats</span>
            </>
          )}
          {pending && <span aria-hidden="true"> · </span>}
          {pending && <span>planned</span>}
          <Verification source={transaction.source} pending={pending} />
          {pending && due && (
            <button
              type="button"
              className="happened"
              disabled={confirm.isPending}
              onClick={() =>
                // The row sent back whole, as it is, with only its status changed: the
                // amount is the server's own rendering, which it reads back unchanged.
                confirm.mutate({
                  id: transaction.id,
                  edit: {
                    direction: transaction.direction,
                    amount: transaction.amount.display,
                    booked_at: transaction.booked_at,
                    label: transaction.label,
                    counterparty: transaction.counterparty,
                    note: transaction.note,
                    status: "confirmed",
                    category_id: transaction.category_id,
                  },
                })
              }
            >
              {confirm.isPending ? "Saving" : transaction.direction === "in" ? "Mark as received" : "Mark as paid"}
            </button>
          )}
        </div>
      </div>
      <div className="figures">
        <div className={transaction.direction === "in" ? "amt in" : "amt"}>
          {signed(transaction.amount, transaction.direction)}
        </div>
        <div className={isNegative(balance_after) ? "after short" : "after"}>
          <span className="visually-hidden">balance after </span>
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
                { onSuccess: () => setOpen("none") },
              )
            }
          />
          <FormError error={setCategory.error} />
        </div>
      )}
      {confirm.error ? (
        <div className="refile">
          <FormError error={confirm.error} />
        </div>
      ) : null}
      {editing && (
        <div className="refile" id={editorId}>
          <RowEditor
            row={transaction}
            categories={categories}
            onDone={() => setOpen("none")}
            onDeleted={onDeleted}
          />
        </div>
      )}
    </li>
  );
}

/**
 * How the row got here. Every settled row says: a hand-entered one with the by-hand mark, a
 * parsed one a person checked in the inbox with the stamp. A row nobody has settled yet —
 * an expected one — carries no mark until it happens.
 */
function Verification({ source, pending }: { source: string; pending: boolean }) {
  if (pending) return null;
  if (source === "manual") return <span className="mt-byhand">By hand</span>;
  // Read off a statement by the parser and accepted by a person: that check is the stamp.
  if (source === "csv" || source === "ofx") return <span className="mt-stamp">Imported</span>;
  // A correction to a count is a person's act too, so it wears the by-hand register; it says
  // "Counted" because that is what the person did, and why the row exists.
  if (source === "adjustment") return <span className="mt-byhand">Adjustment</span>;
  // Written by a series and then confirmed by a person: the confirming is the human act.
  if (source === "recurring") return <span className="mt-byhand">Confirmed</span>;
  return null;
}
