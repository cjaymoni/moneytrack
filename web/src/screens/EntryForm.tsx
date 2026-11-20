import { useRef, useState, type FormEvent } from "react";
import type { Account, Direction } from "../api/types";
import { useCreateTransaction } from "../api/queries";
import { FormError } from "../ui/states";

/** Today's date as an `<input type="date">` writes it, in the reader's own zone. */
function localDate(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/**
 * The instant to book a row at, from the date the person picked.
 *
 * Today means now — the server's own default, so it is left out. Any other day means midday
 * on that day in the reader's zone: far enough from midnight that no zone moves it onto a
 * neighbouring date.
 */
function bookedAt(picked: string, today: string): string | undefined {
  if (!picked || picked === today) return undefined;
  const [year, month, day] = picked.split("-").map(Number);
  if (!year || !month || !day) return undefined;
  return new Date(year, month - 1, day, 12, 0, 0).toISOString();
}

/** One line to record a row: out or in, how much, what it was. */
export function EntryForm({ account }: { account: Account }) {
  const create = useCreateTransaction();
  const amountRef = useRef<HTMLInputElement>(null);
  const today = localDate(new Date());

  const [direction, setDirection] = useState<Direction>("out");
  const [amount, setAmount] = useState("");
  const [label, setLabel] = useState("");
  const [counterparty, setCounterparty] = useState("");
  const [date, setDate] = useState(today);
  const [expected, setExpected] = useState(false);

  function pickDate(value: string) {
    setDate(value);
    // A day still to come is a row still expected. The box stays the person's to change.
    setExpected(value > today);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const at = bookedAt(date, today);
    const who = counterparty.trim();
    create.mutate(
      {
        account_id: account.id,
        direction,
        amount: amount.trim(),
        label: label.trim(),
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
          amountRef.current?.focus();
        },
      },
    );
  }

  return (
    <form className="entry" onSubmit={submit} aria-label="Record a row">
      <div className="entry__top">
        <div className="segments segments--pair" role="radiogroup" aria-label="Direction">
          {(["out", "in"] as const).map((option) => (
            <label key={option} className="segment">
              <input
                type="radio"
                name="direction"
                value={option}
                checked={direction === option}
                onChange={() => setDirection(option)}
              />
              <span>{option === "out" ? "Out" : "In"}</span>
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

      <div className="entry__fields">
        <label className="field">
          <span className="mt-label">What it was</span>
          <input
            required
            maxLength={200}
            placeholder={direction === "out" ? "Trotro, Madina–Circle" : "Salary"}
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
          <span className="mt-label">Day</span>
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
          <span>Expected — it has not happened yet</span>
        </label>
        <button type="submit" className="btn btn--primary entry__submit" disabled={create.isPending}>
          {create.isPending ? "Recording" : "Record"}
        </button>
      </div>

      <FormError error={create.error} />
    </form>
  );
}
