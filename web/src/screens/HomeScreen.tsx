import { Link, Navigate } from "react-router";
import type { Account, Breakdown, Budget } from "../api/types";
import {
  useAccounts,
  useBreakdown,
  useBudgets,
  useInboxCount,
  useOutlook,
  useTotals,
} from "../api/queries";
import { KIND_NAMES, balance, gutterDate, isNegative, longDate, signed } from "../ui/format";
import { budgetsToWatch, shortfalls, upcoming, type Shortfall } from "../ui/home";
import { Failure, Loading } from "../ui/states";
import { monthName, monthOf } from "./BreakdownScreen";

/** How far ahead "Coming up" looks, and how many payments it lists. */
const AHEAD_DAYS = 30;
const AHEAD_SHOWN = 5;
/** How many spending categories the month summary names. */
const TOP_SPENDING = 3;

/**
 * The first page after signing in: what you have, how this month is going, and what needs
 * you — lines to review, an account about to go below zero, a budget near its limit.
 *
 * It adds nothing up itself. Every figure is one the server already worked out for another
 * page; this one puts them side by side.
 */
export function HomeScreen() {
  const accounts = useAccounts();

  if (accounts.isPending) return <Loading />;
  if (accounts.isError) return <Failure error={accounts.error} retry={() => void accounts.refetch()} />;
  // Nothing else here means anything without an account, and adding one is the only next step.
  if (accounts.data.length === 0) return <Navigate to="/accounts/new" replace />;

  return (
    <section className="screen home">
      <h1 className="screen__title">Home</h1>
      <NeedsYou />
      <YouHave accounts={accounts.data} />
      <ThisMonth />
      <ComingUp />
      <Budgets />
    </section>
  );
}

/** Things to act on, first: lines to review and accounts heading below zero. */
function NeedsYou() {
  const count = useInboxCount();
  const outlook = useOutlook(AHEAD_DAYS);
  const waiting = count.data?.total ?? 0;
  const short = outlook.data ? shortfalls(outlook.data) : [];

  if (waiting === 0 && short.length === 0) return null;
  return (
    <div className="home__alerts">
      {waiting > 0 && (
        <p className="review-strip" role="status">
          <span>
            {waiting} imported {waiting === 1 ? "transaction needs" : "transactions need"} a quick check
            before {waiting === 1 ? "it goes" : "they go"} into your accounts.
          </span>
          <Link to="/inbox">Review now</Link>
        </p>
      )}
      {short.map((each) => (
        <ShortfallLine key={each.accountId} shortfall={each} />
      ))}
    </div>
  );
}

function ShortfallLine({ shortfall }: { shortfall: Shortfall }) {
  const amount = `${balance(shortfall.low)} ${shortfall.currency}`;
  return (
    <p className="verdict verdict--short home__alert" role="alert">
      <span>
        {shortfall.already
          ? `${shortfall.accountName} is below zero: ${amount}.`
          : shortfall.on && shortfall.cause
            ? `${shortfall.accountName} goes below zero on ${longDate(shortfall.on)} when ${shortfall.cause} is paid, to ${amount}.`
            : `${shortfall.accountName} goes below zero in the next ${AHEAD_DAYS} days, to ${amount}.`}
      </span>
      <Link to="/outlook?days=30">See the forecast</Link>
    </p>
  );
}

/** Your money now: one total per currency, then each account. */
function YouHave({ accounts }: { accounts: Account[] }) {
  const totals = useTotals();

  return (
    <section className="home__block" aria-labelledby="home-have">
      <h2 id="home-have" className="home__heading">
        You have
      </h2>
      {totals.isError && <Failure error={totals.error} retry={() => void totals.refetch()} />}
      {totals.data && (
        <div className="home__totals">
          {totals.data.map((total) => (
            <p key={total.currency} className="balance__fig">
              <span className={isNegative(total) ? "short" : undefined}>{balance(total)}</span>
              <span className="balance__cur">{total.currency}</span>
            </p>
          ))}
        </div>
      )}
      <ul className="home__accounts">
        {accounts.map((account) => (
          <li key={account.id}>
            <Link to={`/accounts/${account.id}`} className="home__account">
              <span>
                {account.name} <span className="home__muted">· {KIND_NAMES[account.kind]}</span>
              </span>
              <span className={isNegative(account.confirmed) ? "short" : undefined}>
                {balance(account.confirmed)} <span className="home__muted">{account.currency}</span>
              </span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** This calendar month so far: in, out, what's left, and where most of it went. */
function ThisMonth() {
  const month = monthOf(new Date());
  const breakdown = useBreakdown(month);

  return (
    <section className="home__block" aria-labelledby="home-month">
      <h2 id="home-month" className="home__heading">
        {monthName(month)} so far
      </h2>
      {breakdown.isPending && <Loading />}
      {breakdown.isError && <Failure error={breakdown.error} retry={() => void breakdown.refetch()} />}
      {breakdown.data?.map((each) => <MonthSummary key={each.currency} breakdown={each} />)}
      <Link to="/breakdown" className="home__more">
        See all spending
      </Link>
    </section>
  );
}

function MonthSummary({ breakdown }: { breakdown: Breakdown }) {
  const { currency, income, spending, net } = breakdown;
  // Only real categories are "where it went"; spending with no category is said apart, as a
  // prompt to file it rather than as a place the money went.
  const top = spending.lines.filter((line) => line.category_id !== null).slice(0, TOP_SPENDING);
  const unfiled = spending.lines.find((line) => line.category_id === null);

  return (
    <div className="home__month">
      <dl className="home__figures">
        <div>
          <dt>Money in</dt>
          <dd className="in">
            {balance(income.total)} <span className="home__muted">{currency}</span>
          </dd>
        </div>
        <div>
          <dt>Money out</dt>
          <dd>
            {balance(spending.total)} <span className="home__muted">{currency}</span>
          </dd>
        </div>
        <div>
          <dt>Difference</dt>
          <dd className={isNegative(net) ? "short" : undefined}>
            {balance(net)} <span className="home__muted">{currency}</span>
          </dd>
        </div>
      </dl>
      {top.length > 0 && (
        <p className="field__help">
          Most went on{" "}
          {top.map((line, index) => (
            <span key={line.category_id ?? "none"}>
              {index > 0 && (index === top.length - 1 ? " and " : ", ")}
              <b>{line.name}</b> ({balance(line.total)})
            </span>
          ))}
          .
        </p>
      )}
      {unfiled && (
        <p className="field__help">
          {balance(unfiled.total)} {currency} of spending has no category yet. Pick one by clicking
          “No category” on a transaction.
        </p>
      )}
    </div>
  );
}

/** The next few planned payments, across every account. */
function ComingUp() {
  const outlook = useOutlook(AHEAD_DAYS);
  const next = outlook.data ? upcoming(outlook.data, AHEAD_SHOWN) : [];

  return (
    <section className="home__block" aria-labelledby="home-ahead">
      <h2 id="home-ahead" className="home__heading">
        Coming up in the next {AHEAD_DAYS} days
      </h2>
      {outlook.isPending && <Loading />}
      {outlook.isError && <Failure error={outlook.error} retry={() => void outlook.refetch()} />}
      {outlook.data && next.length === 0 && (
        <p className="field__help">
          Nothing planned. Add things you pay or get paid regularly, like rent or salary, and they
          will show here. <Link to="/repeating">Add a regular payment</Link>
        </p>
      )}
      {next.length > 0 && (
        <>
          <ol className="ledger__rows" aria-label="Coming up">
            {next.map(({ transaction, accountName }) => (
              <li key={transaction.id} className="mt-row">
                <time className="date" dateTime={transaction.booked_at} title={longDate(transaction.booked_at)}>
                  {gutterDate(transaction.booked_at)}
                </time>
                <div>
                  <div className="label">{transaction.label}</div>
                  <div className="sub">{accountName}</div>
                </div>
                <div className="figures">
                  <div className={transaction.direction === "in" ? "amt in" : "amt"}>
                    {signed(transaction.amount, transaction.direction)}
                  </div>
                </div>
              </li>
            ))}
          </ol>
          <Link to="/outlook?days=30" className="home__more">
            See the forecast
          </Link>
        </>
      )}
    </section>
  );
}

/** Budgets near or over their limit. Silent when there are none to set. */
function Budgets() {
  const budgets = useBudgets();
  if (!budgets.data || budgets.data.length === 0) return null;
  const watch = budgetsToWatch(budgets.data);

  return (
    <section className="home__block" aria-labelledby="home-budgets">
      <h2 id="home-budgets" className="home__heading">
        Budgets
      </h2>
      {watch.length === 0 ? (
        <p className="field__help">
          {budgets.data.length === 1
            ? "Your budget is within its limit."
            : `All ${budgets.data.length} budgets are within their limits.`}
        </p>
      ) : (
        <ul className="home__watch">
          {watch.map((budget) => (
            <BudgetLine key={budget.id} budget={budget} />
          ))}
        </ul>
      )}
      <Link to="/budgets" className="home__more">
        See budgets
      </Link>
    </section>
  );
}

function BudgetLine({ budget }: { budget: Budget }) {
  const now = budget.now;
  if (!now) return null;
  const over = now.level === "over";
  return (
    <li className={over ? "home__watch-line short" : "home__watch-line"}>
      <b>{budget.name}</b>:{" "}
      {over && now.over_by
        ? `over the limit by ${balance(now.over_by)} ${budget.cap.currency}`
        : `${now.used_display} used, ${balance(now.remaining)} ${budget.cap.currency} left`}
    </li>
  );
}
