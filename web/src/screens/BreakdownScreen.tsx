import { useState } from "react";
import { useSearchParams } from "react-router";
import type { Breakdown, BreakdownLine, BreakdownSide } from "../api/types";
import { useBreakdown } from "../api/queries";
import { balance, isNegative } from "../ui/format";
import { Failure, Loading } from "../ui/states";

/** `YYYY-MM` for the month a date falls in, on the reader's clock. */
export function monthOf(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`;
}

/** The month before or after, as `YYYY-MM`. Calendar arithmetic, not money. */
export function shiftMonth(month: string, by: number): string {
  const [year = 1970, number = 1] = month.split("-").map(Number);
  return monthOf(new Date(year, number - 1 + by, 1));
}

/** "November 2026". */
export function monthName(month: string): string {
  const [year = 1970, number = 1] = month.split("-").map(Number);
  return new Intl.DateTimeFormat("en-GB", { month: "long", year: "numeric" }).format(
    new Date(year, number - 1, 1),
  );
}

const MONTH = /^\d{4}-(0[1-9]|1[0-2])$/;

/**
 * Where the money went in a month: what left and what arrived, by category.
 *
 * Every figure and every share is `mt-core`'s. The bars are drawn from the share the server
 * sent; this screen divides nothing by anything except a share by a hundred to make a CSS
 * width, which is a percentage of a bar and not of anyone's money.
 */
export function BreakdownScreen() {
  const [params, setParams] = useSearchParams();
  const asked = params.get("month");
  const thisMonth = monthOf(new Date());
  const month = asked && MONTH.test(asked) ? asked : thisMonth;
  const breakdown = useBreakdown(month);

  const go = (to: string) => setParams(to === thisMonth ? {} : { month: to });

  return (
    <section className="screen">
      <header className="month-bar">
        <h1 className="screen__title">Where it went</h1>
        <nav className="month-nav" aria-label="Month">
          <button
            type="button"
            className="btn btn--ghost month-nav__step"
            onClick={() => go(shiftMonth(month, -1))}
          >
            <span aria-hidden="true">←</span>
            <span className="visually-hidden">Previous month</span>
          </button>
          <span className="mt-label month-nav__name" aria-live="polite">
            {monthName(month)}
          </span>
          <button
            type="button"
            className="btn btn--ghost month-nav__step"
            onClick={() => go(shiftMonth(month, 1))}
            disabled={month >= thisMonth}
          >
            <span aria-hidden="true">→</span>
            <span className="visually-hidden">Next month</span>
          </button>
        </nav>
      </header>

      {breakdown.isPending && <Loading />}
      {breakdown.isError && (
        <Failure error={breakdown.error} retry={() => void breakdown.refetch()} />
      )}
      {breakdown.data && <Currencies all={breakdown.data} month={month} />}
    </section>
  );
}

/** One breakdown per currency held, chosen between when there is more than one. */
function Currencies({ all, month }: { all: Breakdown[]; month: string }) {
  const [chosen, setChosen] = useState(0);

  if (all.length === 0) {
    return <p className="screen__lede">Open an account to see where its money goes.</p>;
  }
  const shown = all[Math.min(chosen, all.length - 1)] ?? all[0];
  if (!shown) return null;

  return (
    <>
      {all.length > 1 && (
        <div className="segments currency-pick" role="radiogroup" aria-label="Currency">
          {all.map((each, index) => (
            <label key={each.currency} className="segment">
              <input
                type="radio"
                name="breakdown-currency"
                checked={index === chosen}
                onChange={() => setChosen(index)}
              />
              <span>{each.currency}</span>
            </label>
          ))}
        </div>
      )}
      <Month breakdown={shown} month={month} />
    </>
  );
}

function Month({ breakdown, month }: { breakdown: Breakdown; month: string }) {
  const { spending, income, net, currency } = breakdown;
  const empty = spending.lines.length === 0 && income.lines.length === 0;

  return (
    <>
      <div className="balance">
        <p className="mt-label">Spent</p>
        <p className="balance__fig">
          <span>{balance(spending.total)}</span>
          <span className="balance__cur">{currency}</span>
        </p>
        <p className="mt-label balance__meta">
          Came in <span className="balance__then">{balance(income.total)}</span> · Net{" "}
          <span className={isNegative(net) ? "balance__then short" : "balance__then"}>
            {balance(net)}
          </span>
        </p>
      </div>

      {empty ? (
        <p className="screen__lede">Nothing recorded in {monthName(month)}.</p>
      ) : (
        <>
          <Side title="Spending" side={spending} kind="out" />
          <Side title="Income" side={income} kind="in" />
          <p className="field__help">
            Transfers between your own accounts are left out: they are neither spending nor
            income. Expected rows are not counted until they happen.
          </p>
        </>
      )}
    </>
  );
}

function Side({ title, side, kind }: { title: string; side: BreakdownSide; kind: "in" | "out" }) {
  if (side.lines.length === 0) return null;
  return (
    <section className="split" aria-label={title}>
      <h2 className="mt-label split__title">
        <span>{title}</span>
        <span className="split__total">{balance(side.total)}</span>
      </h2>
      <ol className="split__lines">
        {side.lines.map((line) => (
          <Line key={line.category_id ?? "none"} line={line} kind={kind} />
        ))}
      </ol>
    </section>
  );
}

function Line({ line, kind }: { line: BreakdownLine; kind: "in" | "out" }) {
  const name = line.name ?? "Uncategorised";
  const ownHasAny = line.children.length > 0 && line.own.minor_units !== 0;

  return (
    <li className="split__line">
      <Bar
        name={name}
        amount={balance(line.total)}
        share={line.share}
        kind={kind}
        muted={line.category_id === null}
      />
      {line.children.length > 0 && (
        <ol className="split__parts">
          {line.children.map((part) => (
            <li key={part.category_id}>
              <Bar name={part.name} amount={balance(part.total)} share={part.share} kind={kind} part />
            </li>
          ))}
          {ownHasAny && (
            <li>
              <Bar
                name={`${name}, not further filed`}
                amount={balance(line.own)}
                share={line.own_share}
                kind={kind}
                part
              />
            </li>
          )}
        </ol>
      )}
    </li>
  );
}

function Bar({
  name,
  amount,
  share,
  kind,
  part = false,
  muted = false,
}: {
  name: string;
  amount: string;
  share?: { basis_points: number; display: string };
  kind: "in" | "out";
  part?: boolean;
  muted?: boolean;
}) {
  const classes = ["bar", part ? "bar--part" : "", muted ? "bar--muted" : ""].join(" ").trim();
  return (
    <div className={classes}>
      <span className="bar__name">{name}</span>
      <span className="bar__share">{share?.display}</span>
      <span className="bar__amt">{amount}</span>
      {share && (
        <span className="bar__track" aria-hidden="true">
          <span
            className={kind === "in" ? "bar__fill bar__fill--in" : "bar__fill"}
            style={{ width: `${share.basis_points / 100}%` }}
          />
        </span>
      )}
    </div>
  );
}
