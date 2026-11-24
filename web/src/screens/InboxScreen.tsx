import { useState } from "react";
import { Link } from "react-router";
import type { Category, InboxItem, Suspect } from "../api/types";
import { useAcceptInbox, useCategories, useInbox, useMergeInbox, useRejectInbox } from "../api/queries";
import { CategoryPicker, kindFor } from "../ui/CategoryPicker";
import { gutterDate, longDate, signed } from "../ui/format";
import { Failure, FormError, Loading } from "../ui/states";

/** Why a row looks like a duplicate, in the inbox's words. */
const SIGNAL_WORDS: Record<string, string> = {
  same_external_id: "the same bank reference",
  same_amount: "the same amount",
  same_account: "the same account",
  close_in_time: "within a day",
  same_counterparty: "the same payee",
  overlapping_counterparty: "a similar payee",
  same_label: "the same description",
};

/** The reasons for a suspicion as one phrase: "the same amount, within a day and the same payee". */
export function reasons(signals: string[]): string {
  const words = signals.map((signal) => SIGNAL_WORDS[signal]).filter((word): word is string => Boolean(word));
  if (words.length <= 1) return words.join("");
  return `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}`;
}

/**
 * Statement lines waiting for a person.
 *
 * Every card is unverified until someone acts on it, and each action settles it for good:
 * accepted becomes a row, merged was a row already, rejected was never a movement. The
 * category is the server's proposal until the person changes it; changing it teaches the
 * server where that payee belongs.
 */
export function InboxScreen() {
  const inbox = useInbox();
  const categories = useCategories();
  const accept = useAcceptInbox();
  // The person's choice of category per line, where it differs from the proposal.
  const [chosen, setChosen] = useState<Record<string, string | null>>({});

  if (inbox.isPending) return <Loading />;
  if (inbox.isError) return <Failure error={inbox.error} retry={() => void inbox.refetch()} />;

  const { items, total } = inbox.data;
  const categoryFor = (item: InboxItem) => (item.id in chosen ? (chosen[item.id] ?? null) : item.category_id);
  // A line that may be a row already, or may be the same movement as a line from another
  // upload, needs a look of its own: accepting it unseen could book the movement twice.
  const clear = items.filter((item) => item.suspects.length === 0 && item.twins.length === 0);

  return (
    <section className="screen">
      <div className="screen__head">
        <h1 className="screen__title">To review</h1>
        <p className="screen__lede">
          Transactions from statements you imported. Check each one, pick a category and add it.
          Nothing goes into your accounts until you do.
        </p>
      </div>

      {total === 0 && (
        <p className="screen__lede">
          Nothing to review. <Link to="/import">Import a statement</Link>
        </p>
      )}

      {clear.length > 1 && (
        <div className="inbox__bulk">
          <button
            type="button"
            className="btn btn--primary"
            disabled={accept.isPending}
            onClick={() => accept.mutate(clear.map((item) => ({ id: item.id, category_id: categoryFor(item) })))}
          >
            {accept.isPending ? "Adding" : `Add all ${clear.length} that aren't possible duplicates`}
          </button>
          <span className="field__help">Each goes in with the category shown on its card.</span>
          <FormError error={accept.error} />
        </div>
      )}

      {items.length < total && (
        <p className="field__help">
          Showing the oldest {items.length} of {total}. More appear as you clear these.
        </p>
      )}

      {items.length > 0 && (
        <ol className="review" aria-label="Transactions to review">
          {items.map((item) => (
            <Card
              key={item.id}
              item={item}
              categories={categories.data ?? []}
              categoryId={categoryFor(item)}
              onCategory={(id) => setChosen((now) => ({ ...now, [item.id]: id }))}
            />
          ))}
        </ol>
      )}
    </section>
  );
}

function Card({
  item,
  categories,
  categoryId,
  onCategory,
}: {
  item: InboxItem;
  categories: Category[];
  categoryId: string | null;
  onCategory: (id: string | null) => void;
}) {
  const accept = useAcceptInbox();
  const reject = useRejectInbox();
  const merge = useMergeInbox();
  const busy = accept.isPending || reject.isPending || merge.isPending;
  const where = [item.counterparty, item.account_name, item.file_name && `${item.file_name}, line ${item.line_no}`]
    .filter(Boolean)
    .join(" · ");

  return (
    <li className="review-card">
      <div className="mt-row review-card__row">
        <span className="date" title={longDate(item.booked_at)}>
          {gutterDate(item.booked_at)}
        </span>
        <div>
          <div className="label">{item.label}</div>
          {where && <div className="sub">{where}</div>}
        </div>
        <div className="figures">
          <div className={item.direction === "in" ? "amt in" : "amt"}>{signed(item.amount, item.direction)}</div>
        </div>
      </div>

      <CategoryPicker
        categories={categories}
        kind={kindFor(item.direction)}
        value={categoryId}
        onChange={onCategory}
        label={`Category for ${item.label}`}
        disabled={busy}
      />

      {item.suspects.length > 0 && (
        <div className="review-card__suspects">
          <p className="mt-label">Might already be in your accounts</p>
          <ul>
            {item.suspects.map((suspect) => (
              <SuspectRow
                key={suspect.transaction.id}
                suspect={suspect}
                busy={busy}
                onMerge={() => merge.mutate({ id: item.id, transactionId: suspect.transaction.id })}
              />
            ))}
          </ul>
        </div>
      )}

      {item.twins.length > 0 && (
        <div className="review-card__suspects">
          <p className="mt-label">Also in another imported file</p>
          <ul>
            {item.twins.map((twin) => (
              <li key={twin.id} className="review-card__suspect">
                <span>
                  <b>{twin.label}</b>
                  {twin.file_name ? `, ${twin.file_name} line ${twin.line_no}` : ""}
                  <span className="field__help"> — if they're the same payment, ignore one of them.</span>
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="review-card__actions">
        <button
          type="button"
          className="btn btn--ghost"
          disabled={busy}
          onClick={() => reject.mutate(item.id)}
        >
          {reject.isPending ? "Ignoring" : "Ignore"}
        </button>
        <button
          type="button"
          className="btn btn--grade"
          disabled={busy}
          onClick={() => accept.mutate([{ id: item.id, category_id: categoryId }])}
        >
          {accept.isPending ? "Adding" : item.suspects.length > 0 ? "Add as new" : "Add"}
        </button>
      </div>
      <FormError error={accept.error ?? reject.error ?? merge.error} />
    </li>
  );
}

function SuspectRow({ suspect, busy, onMerge }: { suspect: Suspect; busy: boolean; onMerge: () => void }) {
  const row = suspect.transaction;
  return (
    <li className="review-card__suspect">
      <span>
        <b>{row.label}</b>, {longDate(row.booked_at)}, {signed(row.amount, row.direction)}
        {row.status === "pending" ? " (planned)" : ""}
        <span className="field__help">
          {" "}
          — {suspect.certain ? "the bank's reference number matches" : reasons(suspect.signals)}
        </span>
      </span>
      <button type="button" className="btn btn--ghost" disabled={busy} onClick={onMerge}>
        Same payment
      </button>
    </li>
  );
}
