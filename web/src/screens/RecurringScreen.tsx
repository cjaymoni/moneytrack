import { useState, type FormEvent } from "react";
import type { Direction, Frequency, Recurring } from "../api/types";
import { useAccounts, useCategories, useRecurring, useStartRecurring, useStopRecurring } from "../api/queries";
import { CategoryPicker, kindFor } from "../ui/CategoryPicker";
import { localDate } from "../ui/dates";
import { longDate, signed } from "../ui/format";
import { Failure, FormError, Loading } from "../ui/states";

const FREQUENCIES: { value: Frequency; name: string }[] = [
  { value: "weekly", name: "Weekly" },
  { value: "monthly", name: "Monthly" },
  { value: "yearly", name: "Yearly" },
  { value: "daily", name: "Daily" },
];

const UNITS: Record<Frequency, [string, string]> = {
  daily: ["day", "days"],
  weekly: ["week", "weeks"],
  monthly: ["month", "months"],
  yearly: ["year", "years"],
};

const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/** "1st", "22nd", "31st". */
function ordinal(day: number): string {
  const tens = day % 100;
  if (tens >= 11 && tens <= 13) return `${day}th`;
  return `${day}${({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[day % 10] ?? "th"}`;
}

/** A local `YYYY-MM-DD` as a Date at noon, so no zone moves it to a neighbouring day. */
function noonOf(day: string): Date {
  const [y = 1970, m = 1, d = 1] = day.split("-").map(Number);
  return new Date(y, m - 1, d, 12);
}

/** How a series repeats, as a person would say it: "Every month on the 1st". */
export function cadence(freq: Frequency, interval: number, startsOn: string): string {
  const first = noonOf(startsOn);
  const every = (one: string, many: string) => (interval === 1 ? `Every ${one}` : `Every ${interval} ${many}`);
  switch (freq) {
    case "daily":
      return every("day", "days");
    case "weekly":
      return `${every("week", "weeks")} on ${WEEKDAYS[first.getDay()]}`;
    case "monthly": {
      const day = first.getDate();
      return `${every("month", "months")} on the ${ordinal(day)}${day > 28 ? ", or the last day of a shorter month" : ""}`;
    }
    case "yearly":
      return `${every("year", "years")} on ${first.toLocaleDateString("en-GB", { day: "numeric", month: "long" })}`;
  }
}

/**
 * Repeating rows: rent, salary, school fees, the weekly market.
 *
 * A series writes real expected rows a year ahead, which is how the outlook and the budgets
 * see them. Every date and amount here is the server's; this screen names the cadence.
 */
export function RecurringScreen() {
  const series = useRecurring();

  return (
    <section className="screen">
      <div className="screen__head">
        <h1 className="screen__title">Repeating</h1>
        <p className="screen__lede">
          Rows that come round again. Each is written into the ledger a year ahead as expected,
          so the outlook and the budgets can see it coming.
        </p>
      </div>

      <StartSeries />

      {series.isPending && <Loading />}
      {series.isError && <Failure error={series.error} retry={() => void series.refetch()} />}
      {series.data && series.data.length === 0 && (
        <p className="screen__lede">Nothing repeats yet.</p>
      )}
      {series.data && series.data.length > 0 && (
        <ol className="series" aria-label="Repeating rows">
          {series.data.map((each) => (
            <Series key={each.id} series={each} />
          ))}
        </ol>
      )}
    </section>
  );
}

function Series({ series }: { series: Recurring }) {
  const stop = useStopRecurring();
  const [asking, setAsking] = useState(false);

  return (
    <li className="series__row">
      <div className="series__main">
        <div className="series__label">{series.label}</div>
        <div className="series__sub">
          {cadence(series.freq, series.interval, series.starts_on)} · {series.account_name}
          {series.counterparty ? ` · ${series.counterparty}` : ""}
        </div>
        <div className="series__sub">
          {series.next_on ? `Next: ${longDate(`${series.next_on}T12:00:00`)}` : "Ended"}
          {series.ends_on ? ` · until ${longDate(`${series.ends_on}T12:00:00`)}` : ""}
        </div>
      </div>
      <div className={series.direction === "in" ? "series__amt in" : "series__amt"}>
        {signed(series.amount, series.direction)}
      </div>
      <div className="series__actions">
        {asking ? (
          <>
            <span className="field__help">Its expected rows still to come will go. What has happened stays.</span>
            <button type="button" className="btn btn--quiet" onClick={() => setAsking(false)}>
              Keep it
            </button>
            <button
              type="button"
              className="btn btn--ghost row-editor__delete"
              disabled={stop.isPending}
              onClick={() => stop.mutate(series.id)}
            >
              {stop.isPending ? "Stopping" : "Stop repeating"}
            </button>
          </>
        ) : (
          <button type="button" className="btn btn--quiet" onClick={() => setAsking(true)}>
            Stop…
          </button>
        )}
      </div>
      <FormError error={stop.error} />
    </li>
  );
}

function StartSeries() {
  const start = useStartRecurring();
  const accounts = useAccounts();
  const categories = useCategories();
  const today = localDate(new Date());

  const [accountId, setAccountId] = useState("");
  const [direction, setDirection] = useState<Direction>("out");
  const [amount, setAmount] = useState("");
  const [label, setLabel] = useState("");
  const [freq, setFreq] = useState<Frequency>("monthly");
  const [interval, setInterval] = useState(1);
  const [startsOn, setStartsOn] = useState(today);
  const [endsOn, setEndsOn] = useState("");
  const [categoryId, setCategoryId] = useState<string | null>(null);

  const account = accounts.data?.find((each) => each.id === accountId) ?? accounts.data?.[0];
  const chosen = categories.data?.find((category) => category.id === categoryId);

  if (accounts.data && accounts.data.length === 0) {
    return <p className="screen__lede">Open an account first: a repeating row goes into one.</p>;
  }

  function pickDirection(value: Direction) {
    setDirection(value);
    if (value !== direction) setCategoryId(null);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!account) return;
    const words = label.trim() || chosen?.name || "";
    start.mutate(
      {
        account_id: account.id,
        direction,
        amount: amount.trim(),
        label: words,
        freq,
        interval,
        starts_on: startsOn,
        ...(endsOn ? { ends_on: endsOn } : {}),
        ...(chosen ? { category_id: chosen.id } : {}),
      },
      {
        onSuccess: () => {
          setAmount("");
          setLabel("");
          setCategoryId(null);
          setEndsOn("");
        },
      },
    );
  }

  return (
    <form className="entry start-series" onSubmit={submit} aria-label="Start a repeating row">
      <div className="entry__top">
        <div className="segments segments--pair" role="radiogroup" aria-label="Direction">
          {(["out", "in"] as const).map((option) => (
            <label key={option} className="segment">
              <input type="radio" name="series-direction" checked={direction === option} onChange={() => pickDirection(option)} />
              <span>{option === "out" ? "Out" : "In"}</span>
            </label>
          ))}
        </div>
        <label className="entry__amount">
          <span className="visually-hidden">Amount each time, {account?.currency}</span>
          <input
            className="amount-input amount-input--entry"
            inputMode="decimal"
            autoComplete="off"
            placeholder="0.00"
            required
            value={amount}
            onChange={(event) => setAmount(event.target.value)}
          />
          <span className="entry__cur" aria-hidden="true">
            {account?.currency}
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

      <div className="start-series__fields">
        <label className="field">
          <span className="mt-label">What it is</span>
          <input
            required={!chosen}
            maxLength={200}
            placeholder={chosen ? chosen.name : direction === "out" ? "Rent, Spintex" : "Salary"}
            value={label}
            onChange={(event) => setLabel(event.target.value)}
          />
        </label>
        <label className="field">
          <span className="mt-label">Account</span>
          <select value={account?.id ?? ""} onChange={(event) => setAccountId(event.target.value)}>
            {(accounts.data ?? []).map((each) => (
              <option key={each.id} value={each.id}>
                {each.name}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="segments start-series__freq" role="radiogroup" aria-label="How often">
        {FREQUENCIES.map((option) => (
          <label key={option.value} className="segment">
            <input type="radio" name="series-freq" checked={freq === option.value} onChange={() => setFreq(option.value)} />
            <span>{option.name}</span>
          </label>
        ))}
      </div>

      <div className="start-series__fields start-series__when">
        <label className="field">
          <span className="mt-label">First on</span>
          <input type="date" required value={startsOn} onChange={(event) => setStartsOn(event.target.value)} />
        </label>
        <label className="field">
          <span className="mt-label">Every</span>
          <select value={interval} onChange={(event) => setInterval(Number(event.target.value))}>
            {[1, 2, 3, 4, 6].map((n) => (
              <option key={n} value={n}>
                {n === 1 ? UNITS[freq][0] : `${n} ${UNITS[freq][1]}`}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="mt-label">Until</span>
          <input type="date" value={endsOn} min={startsOn} onChange={(event) => setEndsOn(event.target.value)} />
        </label>
      </div>

      <p className="field__help" aria-live="polite">
        {cadence(freq, interval, startsOn)}, from {longDate(`${startsOn}T12:00:00`)}
        {endsOn ? ` until ${longDate(`${endsOn}T12:00:00`)}` : ""}.
        {startsOn < today ? " Days already gone are written as expected rows for you to confirm." : ""}
      </p>

      <FormError error={start.error} />

      <div className="entry__foot">
        <span />
        <button type="submit" className="btn btn--primary entry__submit" disabled={start.isPending}>
          {start.isPending ? "Starting" : "Start repeating"}
        </button>
      </div>
    </form>
  );
}
