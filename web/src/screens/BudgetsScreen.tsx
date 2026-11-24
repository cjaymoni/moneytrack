import { useState, type FormEvent } from "react";
import type { Budget, BudgetPeriod, Standing } from "../api/types";
import {
  useAccounts,
  useBudgets,
  useCategories,
  useChangeBudget,
  useCreateBudget,
  useDeleteBudget,
} from "../api/queries";
import { ordered, fullName } from "../ui/CategoryPicker";
import { localDate } from "../ui/dates";
import { balance, longDate } from "../ui/format";
import { Failure, FormError, Loading } from "../ui/states";

const PERIODS: { value: BudgetPeriod; name: string; per: string }[] = [
  { value: "weekly", name: "Weekly", per: "a week" },
  { value: "monthly", name: "Monthly", per: "a month" },
  { value: "yearly", name: "Yearly", per: "a year" },
];

const WARNINGS = [
  { bps: 0, name: "Never" },
  { bps: 5_000, name: "At 50%" },
  { bps: 7_000, name: "At 70%" },
  { bps: 8_000, name: "At 80%" },
  { bps: 9_000, name: "At 90%" },
];

/** The tiers up to the cap, and the cells past it. */
const TIERS = 10;
/** Past the cap the row runs on for at most this many cells: 200% and 900% both read as far past. */
const MAX_OVER = 10;

/**
 * Budgets: a cap per category, and where each stands.
 *
 * Every figure, every percentage and every level is `mt-core`'s. The tiers are drawn from the
 * share of the cap the server sent, a cell per tenth — a count of cells, not an amount.
 */
export function BudgetsScreen() {
  const budgets = useBudgets();

  if (budgets.isPending) return <Loading />;
  if (budgets.isError) return <Failure error={budgets.error} retry={() => void budgets.refetch()} />;

  const running = budgets.data.filter((budget) => budget.now);
  const earlier = budgets.data.filter((budget) => !budget.now);

  return (
    <section className="screen">
      <div className="screen__head">
        <h1 className="screen__title">Budgets</h1>
        <p className="screen__lede">
          A spending limit for a category, or for everything. A limit on a category includes the
          categories inside it.
        </p>
      </div>

      <AddBudget />

      {running.length === 0 ? (
        <p className="screen__lede">No budgets yet. Set one above.</p>
      ) : (
        <ol className="grades" aria-label="Budgets">
          {running.map((budget) => (
            <BudgetRow key={budget.id} budget={budget} />
          ))}
        </ol>
      )}

      {earlier.length > 0 && (
        <section aria-label="Earlier budgets" className="earlier">
          <h2 className="mt-label">Earlier budgets</h2>
          <ul>
            {earlier.map((budget) => (
              <li key={budget.id} className="field__help">
                {budget.name}: {balance(budget.cap)} {budget.cap.currency}{" "}
                {PERIODS.find((period) => period.value === budget.period)?.per}
                {budget.ends_on ? `, until ${longDate(`${budget.ends_on}T12:00:00`)}` : ""}
              </li>
            ))}
          </ul>
        </section>
      )}
    </section>
  );
}

function BudgetRow({ budget }: { budget: Budget }) {
  const [changing, setChanging] = useState(false);
  const now = budget.now;
  if (!now) return null;
  const over = now.level === "over";

  return (
    <li className="grade">
      <div className="grade__head">
        <button
          type="button"
          className="grade__name"
          aria-expanded={changing}
          onClick={() => setChanging(!changing)}
        >
          {budget.name}
          <span className="visually-hidden">, edit</span>
        </button>
        <span className="grade__value">
          {balance(now.spent)} <span className="grade__of">of {balance(budget.cap)} {budget.cap.currency}</span>
        </span>
      </div>
      <Tiers bps={now.used_bps} />
      <div className="grade__foot">
        <span className={over ? "mt-micro over" : "mt-micro"}>
          {over && now.over_by
            ? `Over by ${balance(now.over_by)}`
            : `${balance(now.remaining)} left, ${daysLeft(now)} to go`}
        </span>
        <span className={over ? "mt-micro over" : "mt-micro"}>
          {now.used_display} of the limit used
          {now.level === "warning" ? " · close to the limit" : ""}
        </span>
      </div>
      <Expected now={now} expected={budget.expected} />
      {changing && <ChangeBudget budget={budget} onDone={() => setChanging(false)} />}
    </li>
  );
}

/** Ten cells to the cap, the cap drawn, and the overspend running past it. */
export function Tiers({ bps }: { bps: number }) {
  const filled = Math.min(Math.floor(bps / 1_000), TIERS);
  const past = Math.max(bps - 10_000, 0);
  const overFull = Math.min(Math.floor(past / 1_000), MAX_OVER);
  const overPart = overFull < MAX_OVER && past % 1_000 > 0;

  return (
    <div className="tiers" aria-hidden="true">
      {Array.from({ length: TIERS }, (_, index) => (
        <i key={index} className={index < filled ? "tier on" : "tier"} />
      ))}
      <i className="cap" />
      {Array.from({ length: overFull }, (_, index) => (
        <i key={`over-${index}`} className="tier over" />
      ))}
      {overPart && <i className="tier over--part" />}
    </div>
  );
}

/** What the rows still expected this period would do, when they change the picture. */
function Expected({ now, expected }: { now: Standing; expected: Standing | null }) {
  if (!expected || expected.spent.minor_units === now.spent.minor_units) return null;
  const worse = expected.level !== now.level;
  return (
    <p className={expected.level === "over" ? "grade__expected over" : "grade__expected"}>
      Planned payments this period take it to {balance(expected.spent)} ({expected.used_display})
      {worse && expected.level === "over" ? " — over the limit" : ""}
      {worse && expected.level === "warning" ? " — close to the limit" : ""}.
    </p>
  );
}

/** "10 days", counting today, to the last day of the period. Calendar arithmetic. */
function daysLeft(standing: Standing): string {
  const [y = 1970, m = 1, d = 1] = standing.period_ends_on.split("-").map(Number);
  const end = new Date(y, m - 1, d);
  const today = new Date(`${localDate(new Date())}T00:00:00`);
  const days = Math.round((end.getTime() - today.getTime()) / 86_400_000) + 1;
  return days === 1 ? "today is the last day" : `${days} days`;
}

function ChangeBudget({ budget, onDone }: { budget: Budget; onDone: () => void }) {
  const change = useChangeBudget();
  const remove = useDeleteBudget();
  const [cap, setCap] = useState(budget.cap.display);
  const [warnAt, setWarnAt] = useState(budget.warn_at_bps);

  function save(event: FormEvent) {
    event.preventDefault();
    change.mutate({ id: budget.id, cap: cap.trim(), warn_at_bps: warnAt }, { onSuccess: () => onDone() });
  }

  return (
    <form className="row-editor grade__editor" onSubmit={save} aria-label={`Change the limit for ${budget.name}`}>
      <div className="row-editor__fields">
        <label className="field">
          <span className="mt-label">Limit, {budget.cap.currency}</span>
          <input className="amount-input" inputMode="decimal" required value={cap} onChange={(event) => setCap(event.target.value)} />
          <span className="field__help">Applies from this period on. Earlier periods keep their old limit.</span>
        </label>
        <label className="field">
          <span className="mt-label">Warn me</span>
          <select value={warnAt} onChange={(event) => setWarnAt(Number(event.target.value))}>
            {WARNINGS.map((option) => (
              <option key={option.bps} value={option.bps}>
                {option.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      <FormError error={change.error ?? remove.error} />
      <div className="row-editor__actions">
        <button
          type="button"
          className="btn btn--ghost row-editor__delete"
          disabled={change.isPending || remove.isPending}
          onClick={() => remove.mutate(budget.id, { onSuccess: () => onDone() })}
        >
          {remove.isPending ? "Removing" : "Remove budget"}
        </button>
        <span className="row-editor__spacer" />
        <button type="button" className="btn btn--quiet" onClick={onDone}>
          Cancel
        </button>
        <button type="submit" className="btn btn--primary" disabled={change.isPending}>
          {change.isPending ? "Saving" : "Save"}
        </button>
      </div>
    </form>
  );
}

function AddBudget() {
  const create = useCreateBudget();
  const categories = useCategories();
  const accounts = useAccounts();
  const [category, setCategory] = useState("");
  const [period, setPeriod] = useState<BudgetPeriod>("monthly");
  const [cap, setCap] = useState("");
  const [warnAt, setWarnAt] = useState(8_000);

  // Caps are per currency and never converted, so the choice is among currencies held.
  const held = [...new Set((accounts.data ?? []).map((account) => account.currency))];
  const [currency, setCurrency] = useState("");
  const chosenCurrency = currency || held[0] || "GHS";
  const spending = ordered(categories.data ?? [], "expense");

  function submit(event: FormEvent) {
    event.preventDefault();
    create.mutate(
      {
        period,
        cap: cap.trim(),
        currency: chosenCurrency,
        warn_at_bps: warnAt,
        ...(category ? { category_id: category } : {}),
      },
      {
        onSuccess: () => {
          setCap("");
          setCategory("");
        },
      },
    );
  }

  return (
    <form className="add-category add-budget" onSubmit={submit} aria-label="Set a budget">
      <div className="segments add-budget__period" role="radiogroup" aria-label="Period">
        {PERIODS.map((option) => (
          <label key={option.value} className="segment">
            <input
              type="radio"
              name="budget-period"
              checked={period === option.value}
              onChange={() => setPeriod(option.value)}
            />
            <span>{option.name}</span>
          </label>
        ))}
      </div>
      <div className="add-budget__fields">
        <label className="field">
          <span className="mt-label">For</span>
          <select value={category} onChange={(event) => setCategory(event.target.value)}>
            <option value="">All spending</option>
            {spending.map((option) => (
              <option key={option.id} value={option.id}>
                {fullName(option, categories.data ?? [])}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="mt-label">Limit</span>
          <input
            className="amount-input"
            inputMode="decimal"
            autoComplete="off"
            placeholder="500"
            required
            value={cap}
            onChange={(event) => setCap(event.target.value)}
          />
        </label>
        {held.length > 1 ? (
          <label className="field">
            <span className="mt-label">Currency</span>
            <select value={chosenCurrency} onChange={(event) => setCurrency(event.target.value)}>
              {held.map((code) => (
                <option key={code}>{code}</option>
              ))}
            </select>
          </label>
        ) : (
          <div className="field">
            <span className="mt-label">Currency</span>
            <span className="add-budget__currency">{chosenCurrency}</span>
          </div>
        )}
        <label className="field">
          <span className="mt-label">Warn me</span>
          <select value={warnAt} onChange={(event) => setWarnAt(Number(event.target.value))}>
            {WARNINGS.map((option) => (
              <option key={option.bps} value={option.bps}>
                {option.name}
              </option>
            ))}
          </select>
        </label>
        <button type="submit" className="btn btn--primary" disabled={create.isPending}>
          {create.isPending ? "Setting" : "Set budget"}
        </button>
      </div>
      <p className="field__help">
        {period === "monthly" && "Counts from the 1st of this month, so spending already done this month is in it."}
        {period === "yearly" && "Counts from 1 January, so spending already done this year is in it."}
        {period === "weekly" && "Each week starts today."}
      </p>
      <FormError error={create.error} />
    </form>
  );
}
