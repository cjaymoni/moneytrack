import { useId } from "react";
import { Link, useSearchParams } from "react-router";
import type { Breakdown, Daily, DaySpend, Outlook, Trend, TrendMonth } from "../api/types";
import { useBreakdown, useDaily, useOutlook, useTrend } from "../api/queries";
import { balance, gutterDate, isNegative } from "../ui/format";
import { axisDay, axisMonth, dayName, tickLabel } from "../ui/charts";
import { Failure, Loading } from "../ui/states";
import { monthName, monthOf } from "./BreakdownScreen";
import { Line } from "./OutlookScreen";
import { usePageTitle } from "../ui/title";
import { InsightsTabs } from "../ui/InsightsTabs";

/** How many months the month-by-month chart can show. */
const RANGES = [6, 12] as const;
const DEFAULT_RANGE = 6;
/** How far ahead the balance lines look. */
const AHEAD_DAYS = 90;
const MONTH = /^\d{4}-(0[1-9]|1[0-2])$/;

/**
 * Your money as pictures: money in and out month by month, one month's spending day by day
 * and by category, and each account's balance over the next three months.
 *
 * Every height and width on this page is one `mt-core` worked out — a level on a shared
 * scale, a share of a total, a point on a forecast. The page turns basis points into CSS
 * percentages and measures no money itself. Each chart has its numbers beside it or one
 * click away, so nothing is only a colour or a shape.
 */
export function ChartsScreen() {
  usePageTitle("Trends");
  const [params, setParams] = useSearchParams();
  const thisMonth = monthOf(new Date());
  const askedRange = Number(params.get("months"));
  const range = (RANGES as readonly number[]).includes(askedRange) ? askedRange : DEFAULT_RANGE;
  const askedMonth = params.get("month");
  const picked = askedMonth && MONTH.test(askedMonth) && askedMonth <= thisMonth ? askedMonth : thisMonth;
  const trend = useTrend(thisMonth, range);

  const update = (changes: { months?: number; month?: string }) => {
    const next = { months: range, month: picked, ...changes };
    const query: Record<string, string> = {};
    if (next.months !== DEFAULT_RANGE) query.months = String(next.months);
    if (next.month !== thisMonth) query.month = next.month;
    setParams(query);
  };

  return (
    <section className="screen charts">
      <InsightsTabs />
      <header className="month-bar">
        <h1 className="screen__title">Trends</h1>
        <div className="segments window-pick" role="radiogroup" aria-label="How many months">
          {RANGES.map((option) => (
            <label key={option} className="segment">
              <input
                type="radio"
                name="charts-months"
                checked={option === range}
                onChange={() => update({ months: option })}
              />
              <span>{option} months</span>
            </label>
          ))}
        </div>
      </header>

      <p className="screen__lede screen__lede--wide">
        Your money over time. For one month split by category, see{" "}
        <Link to="/breakdown">Spending by category</Link>.
      </p>

      {trend.isPending && <Loading />}
      {trend.isError && <Failure error={trend.error} retry={() => void trend.refetch()} />}
      {trend.data && trend.data.length === 0 && (
        <p className="screen__lede">
          Add an account to see charts. <Link to="/accounts/new">Add an account</Link>
        </p>
      )}
      {trend.data?.map((each) => (
        <MonthByMonth
          key={each.currency}
          trend={each}
          picked={picked}
          onPick={(month) => update({ month })}
          showCurrency={trend.data.length > 1}
          isStale={trend.isPlaceholderData}
        />
      ))}
      {trend.data && trend.data.length > 0 && <DayByDay month={picked} />}
      {trend.data && trend.data.length > 0 && <SpendingByCategory month={picked} />}
      {trend.data && trend.data.length > 0 && <BalancesAhead />}
    </section>
  );
}

interface MonthByMonthProps {
  trend: Trend;
  picked: string;
  onPick: (month: string) => void;
  showCurrency: boolean;
  isStale: boolean;
}

/** Money in beside money out for each month, every bar on the one scale the server set. */
function MonthByMonth({ trend, picked, onPick, showCurrency, isStale }: MonthByMonthProps) {
  const headingId = useId();
  const { currency, months, ticks } = trend;
  const isEmpty = months.every((month) => month.income_level_bps === 0 && month.spending_level_bps === 0);

  return (
    <section className="charts__block" aria-labelledby={headingId}>
      <div className="charts__head">
        <h2 id={headingId} className="charts__heading">
          Money in and out{showCurrency ? `, ${currency}` : ""}
        </h2>
        <ul className="legend" aria-label="Key">
          <li>
            <span className="legend__key legend__key--in" aria-hidden="true" />
            Money in
          </li>
          <li>
            <span className="legend__key legend__key--out" aria-hidden="true" />
            Money out
          </li>
        </ul>
      </div>
      <p className="field__help">
        In {currency}. Pick a month to see where its spending went.
      </p>

      {isEmpty ? (
        <p className="field__help">
          Nothing came in or went out in these months. Once you add transactions or{" "}
          <Link to="/import">import a statement</Link>, each month shows here.
        </p>
      ) : (
        <div className={`trend${months.length > 6 ? " trend--dense" : ""}${isStale ? " is-stale" : ""}`}>
          <div className="trend__grid" aria-hidden="true">
            {ticks.map((tick) => (
              <div key={tick.amount.display} className="trend__tick" style={{ bottom: `${tick.level_bps / 100}%` }}>
                <span className="trend__tick-label">{tickLabel(tick.amount)}</span>
              </div>
            ))}
          </div>
          <ol className="trend__months">
            {months.map((month, index) => (
              <MonthColumn
                key={month.month}
                month={month}
                currency={currency}
                isFirst={index === 0}
                isPicked={month.month === picked}
                onPick={() => onPick(month.month)}
              />
            ))}
          </ol>
        </div>
      )}

      <details className="charts__table">
        <summary>Show as a table</summary>
        <table>
          <caption className="visually-hidden">Money in and out by month, in {currency}</caption>
          <thead>
            <tr>
              <th scope="col">Month</th>
              <th scope="col">Money in</th>
              <th scope="col">Money out</th>
              <th scope="col">Difference</th>
            </tr>
          </thead>
          <tbody>
            {months.map((month) => (
              <tr key={month.month}>
                <th scope="row">{monthName(month.month)}</th>
                <td>{balance(month.income)}</td>
                <td>{balance(month.spending)}</td>
                <td className={isNegative(month.net) ? "short" : undefined}>{balance(month.net)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </section>
  );
}

interface MonthColumnProps {
  month: TrendMonth;
  currency: string;
  isFirst: boolean;
  isPicked: boolean;
  onPick: () => void;
}

function MonthColumn({ month, currency, isFirst, isPicked, onPick }: MonthColumnProps) {
  const name = monthName(month.month);
  const moneyIn = `${balance(month.income)} ${currency}`;
  const moneyOut = `${balance(month.spending)} ${currency}`;
  const difference = `${balance(month.net)} ${currency}`;

  return (
    <li className="trend__month">
      <button
        type="button"
        className="trend__hit"
        aria-pressed={isPicked}
        aria-label={`${name}: money in ${moneyIn}, money out ${moneyOut}`}
        onClick={onPick}
      >
        <span className="trend__bars" aria-hidden="true">
          <span className="trend__bar trend__bar--in" style={{ height: `${month.income_level_bps / 100}%` }} />
          <span className="trend__bar trend__bar--out" style={{ height: `${month.spending_level_bps / 100}%` }} />
        </span>
        <span className="trend__label" aria-hidden="true">
          {axisMonth(month.month, isFirst)}
        </span>
        <span className="trend__tip" aria-hidden="true">
          <span className="trend__tip-title">{name}</span>
          <span className="trend__tip-row">
            <span className="trend__tip-key trend__tip-key--in" />
            <b>{moneyIn}</b> in
          </span>
          <span className="trend__tip-row">
            <span className="trend__tip-key trend__tip-key--out" />
            <b>{moneyOut}</b> out
          </span>
          <span className="trend__tip-row">
            <span className="trend__tip-key" />
            <b className={isNegative(month.net) ? "short" : undefined}>{difference}</b> difference
          </span>
        </span>
      </button>
    </li>
  );
}

/** The picked month's spending, one bar per date, on the scale the server set for it. */
function DayByDay({ month }: { month: string }) {
  const headingId = useId();
  const daily = useDaily(month);

  return (
    <section className="charts__block" aria-labelledby={headingId}>
      <h2 id={headingId} className="charts__heading">
        Spending day by day in {monthName(month)}
      </h2>
      {daily.isPending && <Loading />}
      {daily.isError && <Failure error={daily.error} retry={() => void daily.refetch()} />}
      {daily.data?.map((each) => (
        <DayBars key={each.currency} daily={each} isStale={daily.isPlaceholderData} />
      ))}
    </section>
  );
}

function DayBars({ daily, isStale }: { daily: Daily; isStale: boolean }) {
  const { currency, days, ticks, total } = daily;
  // Whether anything left that day is a sign, not a sum.
  const spent = days.filter((day) => day.spending.minor_units > 0);
  if (spent.length === 0) {
    return <p className="field__help">Nothing spent in {currency} this month.</p>;
  }
  // The tallest bar is the server's measure; comparing heights compares no money.
  const biggest = spent.reduce((top, day) => (day.level_bps > top.level_bps ? day : top));

  return (
    <div className="ranked-wrap">
      <p className="field__help">
        {balance(total)} {currency} in all, on {spent.length} {spent.length === 1 ? "day" : "days"}. The most
        was {balance(biggest.spending)} on {dayName(biggest.date)}.
      </p>
      <div className={`trend trend--days${isStale ? " is-stale" : ""}`} aria-hidden="true">
        <div className="trend__grid">
          {ticks.map((tick) => (
            <div key={tick.amount.display} className="trend__tick" style={{ bottom: `${tick.level_bps / 100}%` }}>
              <span className="trend__tick-label">{tickLabel(tick.amount)}</span>
            </div>
          ))}
        </div>
        <ol className="trend__months">
          {days.map((day) => (
            <DayColumn key={day.date} day={day} currency={currency} />
          ))}
        </ol>
      </div>
      <details className="charts__table">
        <summary>Show as a table</summary>
        <table>
          <caption className="visually-hidden">Days with spending, in {currency}</caption>
          <thead>
            <tr>
              <th scope="col">Date</th>
              <th scope="col">Money out</th>
            </tr>
          </thead>
          <tbody>
            {spent.map((day) => (
              <tr key={day.date}>
                <th scope="row">{dayName(day.date)}</th>
                <td>{balance(day.spending)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </div>
  );
}

/** One date's bar. Hover shows the figure; the table carries it for everyone else. */
function DayColumn({ day, currency }: { day: DaySpend; currency: string }) {
  return (
    <li className="trend__month">
      <div className="trend__hit">
        <span className="trend__bars">
          <span className="trend__bar trend__bar--out" style={{ height: `${day.level_bps / 100}%` }} />
        </span>
        <span className="trend__label">{axisDay(day.date)}</span>
        <span className="trend__tip">
          <span className="trend__tip-title">{dayName(day.date)}</span>
          <span className="trend__tip-row">
            <span className="trend__tip-key trend__tip-key--out" />
            <b>
              {balance(day.spending)} {currency}
            </b>{" "}
            out
          </span>
        </span>
      </div>
    </li>
  );
}

/** The picked month's spending, a bar per category, each as wide as its share of the total. */
function SpendingByCategory({ month }: { month: string }) {
  const headingId = useId();
  const breakdown = useBreakdown(month);

  return (
    <section className="charts__block" aria-labelledby={headingId}>
      <h2 id={headingId} className="charts__heading">
        Where the money went in {monthName(month)}
      </h2>
      {breakdown.isPending && <Loading />}
      {breakdown.isError && <Failure error={breakdown.error} retry={() => void breakdown.refetch()} />}
      {breakdown.data?.map((each) => (
        <CategoryBars key={each.currency} breakdown={each} month={month} />
      ))}
    </section>
  );
}

function CategoryBars({ breakdown, month }: { breakdown: Breakdown; month: string }) {
  const { currency, spending } = breakdown;
  if (spending.lines.length === 0) {
    return <p className="field__help">No spending in {currency} this month.</p>;
  }

  return (
    <div className="ranked-wrap">
      <p className="field__help">
        {balance(spending.total)} {currency} in all. Each bar is that category's share.
      </p>
      <ol className="ranked" aria-label={`Spending by category in ${currency}`}>
        {spending.lines.map((line) => (
          <li key={line.category_id ?? "none"} className="ranked__row">
            {line.category_id ? (
              <span className="ranked__name">{line.name}</span>
            ) : (
              <Link to="/sort" className="ranked__name ranked__name--none">
                No category — sort these
              </Link>
            )}
            <span className="ranked__amt">
              {balance(line.total)} <span className="ranked__share">{line.share.display}</span>
            </span>
            <span className="ranked__track" aria-hidden="true">
              <span className="ranked__fill" style={{ width: `${line.share.basis_points / 100}%` }} />
            </span>
          </li>
        ))}
      </ol>
      <Link to={`/breakdown?month=${month}`} className="home__more">
        See this month in detail
      </Link>
    </div>
  );
}

/** Each account's balance over the next three months, as the forecast draws it. */
function BalancesAhead() {
  const headingId = useId();
  const outlook = useOutlook(AHEAD_DAYS);

  return (
    <section className="charts__block" aria-labelledby={headingId}>
      <h2 id={headingId} className="charts__heading">
        Balance over the next {AHEAD_DAYS} days
      </h2>
      {outlook.isPending && <Loading />}
      {outlook.isError && <Failure error={outlook.error} retry={() => void outlook.refetch()} />}
      {outlook.data?.map((each) => <AccountLine key={each.account.id} outlook={each} />)}
      {outlook.data && outlook.data.length > 0 && (
        <Link to="/outlook" className="home__more">
          See the forecast
        </Link>
      )}
    </section>
  );
}

function AccountLine({ outlook }: { outlook: Outlook }) {
  const { account } = outlook;
  const lowRow = outlook.points.find((point) => point.transaction.id === outlook.low_transaction_id);
  const low = `${balance(outlook.low)} ${account.currency}`;

  return (
    <div className="charts__line">
      <p className="charts__line-head">
        <b>{account.name}</b>
        <span className="home__muted">
          {" "}
          · now {balance(outlook.start)} {account.currency}, lowest{" "}
          <span className={isNegative(outlook.low) ? "short" : undefined}>{low}</span>
          {lowRow ? ` on ${gutterDate(lowRow.transaction.booked_at)}` : " today"}
        </span>
      </p>
      <Line outlook={outlook} />
    </div>
  );
}
