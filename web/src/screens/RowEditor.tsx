import { useState, type FormEvent } from "react";
import type { Category, Direction, Transaction } from "../api/types";
import { useDeleteTransaction, useEditTransaction } from "../api/queries";
import { CategoryPicker, kindFor } from "../ui/CategoryPicker";
import { keptOrMoved, localDate, localDateOf } from "../ui/dates";
import { FormError } from "../ui/states";

/**
 * Changing one row, in place under it: everything a person typed, and a way to delete it.
 *
 * The amount starts as the server's own rendering of it — `12.50`, the same text a person
 * would type — and goes back as text for `mt-core` to read. Nothing here works it out.
 */
export function RowEditor({
  row,
  categories,
  onDone,
  onDeleted,
}: {
  row: Transaction;
  categories: Category[];
  onDone: () => void;
  onDeleted: (row: Transaction) => void;
}) {
  const edit = useEditTransaction();
  const remove = useDeleteTransaction();
  const today = localDate(new Date());

  const [direction, setDirection] = useState<Direction>(row.direction);
  const [amount, setAmount] = useState(row.amount.display);
  const [label, setLabel] = useState(row.label);
  const [counterparty, setCounterparty] = useState(row.counterparty ?? "");
  const [note, setNote] = useState(row.note ?? "");
  const [date, setDate] = useState(localDateOf(row.booked_at));
  const [expected, setExpected] = useState(row.status === "pending");
  const [categoryId, setCategoryId] = useState<string | null>(row.category_id);
  const busy = edit.isPending || remove.isPending;

  function pickDirection(value: Direction) {
    setDirection(value);
    // A spending category does not carry over to money in, or the other way.
    if (value !== direction) setCategoryId(null);
  }

  function pickDate(value: string) {
    setDate(value);
    if (value > today) setExpected(true);
  }

  function save(event: FormEvent) {
    event.preventDefault();
    const who = counterparty.trim();
    const words = note.trim();
    edit.mutate(
      {
        id: row.id,
        edit: {
          direction,
          amount: amount.trim(),
          booked_at: keptOrMoved(row.booked_at, date),
          label: label.trim(),
          counterparty: who || null,
          note: words || null,
          status: expected ? "pending" : "confirmed",
          category_id: categoryId,
        },
      },
      { onSuccess: () => onDone() },
    );
  }

  return (
    <form className="row-editor" onSubmit={save} aria-label={`Change ${row.label}`}>
      <div className="row-editor__top">
        <div className="segments segments--pair" role="radiogroup" aria-label="Direction">
          {(["out", "in"] as const).map((option) => (
            <label key={option} className="segment">
              <input
                type="radio"
                name={`direction-${row.id}`}
                value={option}
                checked={direction === option}
                onChange={() => pickDirection(option)}
              />
              <span>{option === "out" ? "Money out" : "Money in"}</span>
            </label>
          ))}
        </div>
        <label className="field">
          <span className="mt-label">Amount, {row.amount.currency}</span>
          <input
            className="amount-input"
            inputMode="decimal"
            autoComplete="off"
            required
            value={amount}
            onChange={(event) => setAmount(event.target.value)}
          />
        </label>
        <label className="field">
          <span className="mt-label">Date</span>
          <input type="date" required value={date} onChange={(event) => pickDate(event.target.value)} />
        </label>
      </div>

      <CategoryPicker
        categories={categories}
        kind={kindFor(direction)}
        value={categoryId}
        onChange={setCategoryId}
        label="Category"
      />

      <div className="row-editor__fields">
        <label className="field">
          <span className="mt-label">Description</span>
          <input required maxLength={200} value={label} onChange={(event) => setLabel(event.target.value)} />
        </label>
        <label className="field">
          <span className="mt-label">{direction === "out" ? "Paid to" : "From"}</span>
          <input
            maxLength={120}
            placeholder="Optional"
            value={counterparty}
            onChange={(event) => setCounterparty(event.target.value)}
          />
        </label>
      </div>
      <label className="field">
        <span className="mt-label">Note</span>
        <input maxLength={2000} placeholder="Optional" value={note} onChange={(event) => setNote(event.target.value)} />
      </label>

      <label className="check">
        <input type="checkbox" checked={expected} onChange={(event) => setExpected(event.target.checked)} />
        <span>Planned — hasn't happened yet</span>
      </label>

      <FormError error={edit.error ?? remove.error} />

      <div className="row-editor__actions">
        <button
          type="button"
          className="btn btn--ghost row-editor__delete"
          disabled={busy}
          onClick={() => remove.mutate(row, { onSuccess: (gone) => onDeleted(gone) })}
        >
          {remove.isPending ? "Deleting" : "Delete"}
        </button>
        <span className="row-editor__spacer" />
        <button type="button" className="btn btn--quiet" onClick={onDone} disabled={busy}>
          Cancel
        </button>
        <button type="submit" className="btn btn--primary" disabled={busy}>
          {edit.isPending ? "Saving" : "Save"}
        </button>
      </div>
    </form>
  );
}
