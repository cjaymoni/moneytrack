import { useState, type ReactNode } from "react";
import { Link, useNavigate } from "react-router";
import type { Account, Breakdown, Budget } from "../api/types";
import {
  useAccounts,
  useAllTransactions,
  useBreakdown,
  useBudgets,
  useInboxCount,
  useOutlook,
  useTotals,
} from "../api/queries";
import { KIND_NAMES, balance, gutterDate, isNegative, longDate, plural, signed } from "../ui/format";
import { budgetsToWatch, shortfalls, upcoming, type Shortfall } from "../ui/home";
import { Failure, Loading } from "../ui/states";
import { monthName, monthOf } from "./BreakdownScreen";
import { usePageTitle } from "../ui/title";
import { HowItWorks } from "../ui/HowItWorks";
import { isComplete, readHidden, saveHidden, steps, type Progress } from "../ui/gettingStarted";

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
  usePageTitle("Home");
  const accounts = useAccounts();

  if (accounts.isPending) return <Loading />;
  if (accounts.isError) return <Failure error={accounts.error} retry={() => void accounts.refetch()} />;
  // A new person stays on Home, with a checklist of what to do first, rather than being
  // dropped on a form with no idea what comes after it.
  if (accounts.data.length === 0) {
    return (
      <section className="screen home">
        <div className="screen__head">
          <h1 className="screen__title">Welcome to moneytrack</h1>
          <p className="screen__lede screen__lede--wide">
            Keep track of every cedi in and out — MoMo, bank and cash in one place. Three
            steps get you going.
          </p>
        </div>
        <GetStarted accounts={accounts.data} />
      </section>
    );
  }

  return (
    <section className="screen home">
      <h1 className="screen__title">Home</h1>
      <GetStarted accounts={accounts.data} />
      <QuickActions accounts={accounts.data} />
      <NeedsYou />
      <YouHave accounts={accounts.data} />
      <ThisMonth />
      <ComingUp />
      <Budgets />
    </section>
  );
}

/**
 * The first three things to do, ticked off as they are done, until all are done or the
 * person puts the list away. Each step goes straight to where it is done.
 */
function GetStarted({ accounts }: { accounts: Account[] }) {
  const transactions = useAllTransactions();
  const budgets = useBudgets();
  const [hidden, setHidden] = useState(readHidden);
  const progress: Progress = {
    hasAccount: accounts.length > 0,
    hasTransaction: (transactions.data?.length ?? 0) > 0,
    hasBudget: (budgets.data?.length ?? 0) > 0,
  };
  // Until both answers are in, a step may look undone that is done.
  if (hidden || transactions.isPending || budgets.isPending || isComplete(progress)) return null;
  const done = steps(progress).filter((step) => step.done).length;
  const first = accounts[0];

  return (
    <section className="start" aria-labelledby="start-heading">
      <div className="start__head">
        <h2 id="start-heading" className="home__heading">
          Get started
        </h2>
        <span className="home__muted">{done} of 3 done</span>
      </div>
      <ol className="start__steps">
        <Step done={progress.hasAccount} title="Add an account">
          Your MoMo wallet, a bank account or your cash. <Link to="/accounts/new">Add an account</Link>
        </Step>
        <Step done={progress.hasTransaction} title="Record what came in and went out">
          Add a transaction by hand, or bring in a statement from your bank or MoMo app.{" "}
          {first ? (
            <>
              <Link to={`/accounts/${first.id}?add=1`}>Add a transaction</Link> or{" "}
              <Link to="/import">import a statement</Link>
            </>
          ) : (
            "You can do this once you have an account."
          )}
        </Step>
        <Step done={progress.hasBudget} title="Set a budget">
          A monthly limit, so you can see what's left to spend. <Link to="/budgets">Set a budget</Link>
        </Step>
      </ol>
      <HowItWorks />
      {progress.hasAccount && (
        <button
          type="button"
          className="btn btn--quiet start__hide"
          onClick={() => {
            saveHidden();
            setHidden(true);
          }}
        >
          Hide this list
        </button>
      )}
    </section>
  );
}

function Step({ done, title, children }: { done: boolean; title: string; children: ReactNode }) {
  return (
    <li className={done ? "start__step start__step--done" : "start__step"}>
      <span className="start__mark" aria-hidden="true">
        {done ? "✓" : ""}
      </span>
      <div>
        <p className="start__title">
          {title}
          {done && <span className="visually-hidden"> — done</span>}
        </p>
        {!done && <p className="field__help">{children}</p>}
      </div>
    </li>
  );
}

/**
 * The two things people come to do, as buttons, so nobody has to know which page they live
 * on. With more than one account, adding asks which one first.
 */
function QuickActions({ accounts }: { accounts: Account[] }) {
  const navigate = useNavigate();
  const [into, setInto] = useState(accounts[0]?.id ?? "");
  const target = accounts.some((account) => account.id === into) ? into : (accounts[0]?.id ?? "");

  return (
    <div className="home__actions">
      {accounts.length > 1 && (
        <label className="field home__into">
          <span className="mt-label">Account</span>
          <select value={target} onChange={(event) => setInto(event.target.value)}>
            {accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.name}
              </option>
            ))}
          </select>
        </label>
      )}
      <button
        type="button"
        className="btn btn--primary"
        onClick={() => void navigate(`/accounts/${target}?add=1`)}
      >
        Add a transaction
      </button>
      <Link to="/import" className="btn btn--ghost">
        Import a statement
      </Link>
    </div>
  );
}

/** Things to act on, first: lines to review and accounts heading below zero. */
function NeedsYou() {
  const count = useInboxCount();
  const outlook = useOutlook(AHEAD_DAYS);
  const waiting = count.data?.total ?? 0;
  const short = outlook.data ? shortfalls(outlook.data) : [];

  // One polite live region, always in the page, so a notice that appears is announced;
  // a region added together with its text often is not.
  return (
    <div className="home__alerts live" role="status">
      {waiting > 0 && (
        <p className="review-strip">
          <span>
            {waiting}{" "}
            {plural(waiting, {
              one: "imported transaction needs a quick check before it goes into your accounts.",
              other: "imported transactions need a quick check before they go into your accounts.",
            })}
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
    <p className="verdict verdict--short home__alert">
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
  const budgets = useBudgets();

  return (
    <section className="home__block" aria-labelledby="home-month">
      <h2 id="home-month" className="home__heading">
        {monthName(month)} so far
      </h2>
      {breakdown.isPending && <Loading />}
      {breakdown.isError && <Failure error={breakdown.error} retry={() => void breakdown.refetch()} />}
      {breakdown.data?.map((each) => <MonthSummary key={each.currency} breakdown={each} />)}
      {budgets.data && budgets.data.length === 0 && (
        <p className="field__help">
          Want to know what's left to spend? <Link to="/budgets">Set a monthly budget</Link>
        </p>
      )}
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
          <dt>{isNegative(net) ? "Spent more than came in" : "Left over"}</dt>
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
          {balance(unfiled.total)} {currency} of spending has no category yet.{" "}
          <Link to="/sort">Sort it now</Link>
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
