import { useRef, useState, type FormEvent } from "react";
import type { Account, Direction } from "../api/types";
import { useCategories, useCreateTransaction } from "../api/queries";
import { CategoryPicker, kindFor } from "../ui/CategoryPicker";
import { FormError } from "../ui/states";
import { localDate, middayOn } from "../ui/dates";

/** Today means now — the server's own default, so it is left out. Any other day is midday. */
function bookedAt(picked: string, today: string): string | undefined {
  if (!picked || picked === today) return undefined;
  return middayOn(picked);
}

/**
 * One line to record a row: out or in, how much, which category, what it was.
 *
 * The fast path is amount, category, done. A row with a category and no words of its own
 * is labelled with the category's name — the label is required, and making someone type
 * "Transport" under the Transport chip they just pressed is the slow path the plan warns
 * about.
 */
export function EntryForm({ account }: { account: Account }) {
  const create = useCreateTransaction();
  const categories = useCategories();
  const amountRef = useRef<HTMLInputElement>(null);
  const today = localDate(new Date());

  const [direction, setDirection] = useState<Direction>("out");
  const [amount, setAmount] = useState("");
  const [label, setLabel] = useState("");
  const [counterparty, setCounterparty] = useState("");
  const [date, setDate] = useState(today);
  const [expected, setExpected] = useState(false);
  const [categoryId, setCategoryId] = useState<string | null>(null);

  const chosen = categories.data?.find((category) => category.id === categoryId) ?? null;

  function pickDirection(value: Direction) {
    setDirection(value);
    // A spending category does not carry over to money in, or the other way.
    if (value !== direction) setCategoryId(null);
  }

  function pickDate(value: string) {
    setDate(value);
    // A day still to come is a row still expected. The box stays the person's to change.
    setExpected(value > today);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const at = bookedAt(date, today);
    const who = counterparty.trim();
    const words = label.trim() || chosen?.name || "";
    create.mutate(
      {
        account_id: account.id,
        direction,
        amount: amount.trim(),
        label: words,
        ...(chosen ? { category_id: chosen.id } : {}),
        ...(who ? { counterparty: who } : {}),
        ...(at ? { booked_at: at } : {}),
        status: expected ? "pending" : "confirmed",
      },
      {
        onSuccess: () => {
          setAmount("");
          setLabel("");
          setCounterparty("");
          setDate(today);
          setExpected(false);
          setCategoryId(null);
          amountRef.current?.focus();
        },
      },
    );
  }

  return (
    <form className="entry" onSubmit={submit} aria-label="Add a transaction">
      <div className="entry__top">
        <div className="segments segments--pair" role="radiogroup" aria-label="Direction">
          {(["out", "in"] as const).map((option) => (
            <label key={option} className="segment">
              <input
                type="radio"
                name="direction"
                value={option}
                checked={direction === option}
                onChange={() => pickDirection(option)}
              />
              <span>{option === "out" ? "Money out" : "Money in"}</span>
            </label>
          ))}
        </div>

        <label className="entry__amount">
          <span className="visually-hidden">Amount in {account.currency}</span>
          <input
            ref={amountRef}
            className="amount-input amount-input--entry"
            inputMode="decimal"
            autoComplete="off"
            placeholder="0.00"
            required
            value={amount}
            onChange={(event) => setAmount(event.target.value)}
          />
          <span className="entry__cur" aria-hidden="true">
            {account.currency}
          </span>
        </label>
      </div>

      {categories.data && (
        <CategoryPicker
          categories={categories.data}
          kind={kindFor(direction)}
          value={categoryId}
          onChange={setCategoryId}
          label="Category"
        />
      )}

      <div className="entry__fields">
        <label className="field">
          <span className="mt-label">Description</span>
          <input
            required={!chosen}
            maxLength={200}
            placeholder={chosen ? chosen.name : direction === "out" ? "Trotro, Madina–Circle" : "Salary"}
            value={label}
            onChange={(event) => setLabel(event.target.value)}
          />
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
        <label className="field field--date">
          <span className="mt-label">Date</span>
          <input type="date" required value={date} onChange={(event) => pickDate(event.target.value)} />
        </label>
      </div>

      <div className="entry__foot">
        <label className="check">
          <input
            type="checkbox"
            checked={expected}
            onChange={(event) => setExpected(event.target.checked)}
          />
          <span>Planned — hasn't happened yet</span>
        </label>
        <button type="submit" className="btn btn--primary entry__submit" disabled={create.isPending}>
          {create.isPending ? "Adding" : "Add"}
        </button>
      </div>

      <FormError error={create.error} />
    </form>
  );
}
