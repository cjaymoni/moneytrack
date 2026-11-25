import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Link, NavLink, Outlet, useLocation } from "react-router";
import type { User } from "../api/types";
import { useAccounts, useAllTransactions, useInboxCount, useSignOut, useTotals } from "../api/queries";
import { KIND_NAMES, balance, isNegative } from "../ui/format";
import { Failure, Loading } from "../ui/states";
import { Icon, type IconName } from "../ui/icons";

/** The frame every signed-in screen sits in: a top bar, the accounts, and the screen. */
export function Shell({ user }: { user: User }) {
  const signOut = useSignOut();
  const { pathname } = useLocation();
  const main = useRef<HTMLElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [shownFor, setShownFor] = useState(pathname);

  // A new page: the folded menu closes behind it, and focus moves to the page so a screen
  // reader announces it rather than staying on the link that was pressed.
  if (shownFor !== pathname) {
    setShownFor(pathname);
    setMenuOpen(false);
  }
  // Compared with the page last shown rather than a "first run" flag, so it behaves the
  // same when React runs effects twice in development.
  const shownPath = useRef(pathname);
  useEffect(() => {
    if (shownPath.current === pathname) return;
    shownPath.current = pathname;
    main.current?.focus();
  }, [pathname]);

  return (
    <div className="shell">
      <a href="#content" className="skip">
        Skip to content
      </a>
      <header className="topbar">
        <div className="topbar__start">
          <button
            type="button"
            className="btn btn--ghost menu-toggle"
            aria-expanded={menuOpen}
            aria-controls="main-menu"
            onClick={() => setMenuOpen((open) => !open)}
          >
            {menuOpen ? "Close menu" : "Menu"}
          </button>
          <Link to="/" className="wordmark" aria-label="moneytrack, go to Home" title="Go to Home">
            moneytrack
          </Link>
        </div>
        <div className="topbar__who">
          <span className="topbar__email">{user.email}</span>
          <button
            type="button"
            className="btn btn--quiet"
            onClick={() => signOut.mutate()}
            disabled={signOut.isPending}
          >
            Sign out
          </button>
        </div>
      </header>

      <nav id="main-menu" className={menuOpen ? "rail rail--open" : "rail"} aria-label="Main">
        <Menu />
      </nav>

      <main id="content" className="main" ref={main} tabIndex={-1}>
        <Outlet />
      </main>
    </div>
  );
}

/** The pages under Insights: one menu item, with tabs on the pages themselves. */
export const INSIGHT_PATHS = ["/breakdown", "/charts", "/outlook"];

/**
 * Where to go, in the order someone needs it: Home and the two chores first, then the
 * accounts — where transactions live — then what to look at, what to plan, and setting up.
 * Small on purpose: three related views share one "Insights" item, and the less used
 * things live in Settings.
 */
function Menu() {
  const { pathname } = useLocation();
  const inInsights = INSIGHT_PATHS.some((path) => pathname.startsWith(path));
  const waiting = useInboxCount().data?.total ?? 0;
  const transactions = useAllTransactions();
  // A count of rows without a category, not of money.
  const unsorted = transactions.data?.filter((row) => row.category_id === null).length ?? 0;

  return (
    <div className="rail__section rail__menu">
      <ul className="rail__list">
        <MenuItem to="/" end icon="home" label="Home" />
        <MenuItem to="/inbox" icon="check" label="Check imports" count={waiting} countLabel="waiting" urgent />
        <MenuItem to="/sort" icon="sort" label="Sort into categories" count={unsorted} countLabel="without a category" />
      </ul>
      <Accounts />
      <MenuGroup title="Your money">
        <MenuItem to="/breakdown" icon="insights" label="Insights" current={inInsights} />
        <MenuItem to="/budgets" icon="budget" label="Budgets" />
        <MenuItem to="/repeating" icon="repeat" label="Regular payments" />
        <MenuItem to="/import" icon="import" label="Import a statement" />
      </MenuGroup>
      <ul className="rail__list rail__list--last">
        <MenuItem to="/settings" icon="settings" label="Settings" />
      </ul>
      <Totals />
    </div>
  );
}

interface MenuItemProps {
  to: string;
  icon: IconName;
  label: string;
  end?: boolean;
  /** Overrides the router's idea of "here", for one item standing for several pages. */
  current?: boolean;
  count?: number;
  /** Said after the count to a screen reader: "3 waiting". */
  countLabel?: string;
  /** Amber, because amber means "awaiting your check" and nothing else. */
  urgent?: boolean;
}

/** One place to go: an icon, its name, and how many things wait there, on one row. */
function MenuItem({ to, icon, label, end = false, current, count = 0, countLabel, urgent = false }: MenuItemProps) {
  const body = (
    <>
      <Icon name={icon} />
      <span className="rail__label">{label}</span>
      {count > 0 && (
        <span className={urgent ? "rail__badge rail__badge--urgent" : "rail__badge"}>
          {count}
          {countLabel && <span className="visually-hidden"> {countLabel}</span>}
        </span>
      )}
    </>
  );
  return (
    <li>
      {current === undefined ? (
        <NavLink to={to} end={end} className="rail__link">
          {body}
        </NavLink>
      ) : (
        <Link to={to} className="rail__link" aria-current={current ? "page" : undefined}>
          {body}
        </Link>
      )}
    </li>
  );
}

function MenuGroup({ title, children }: { title: string; children: ReactNode }) {
  const id = useId();
  return (
    <section className="rail__group" aria-labelledby={id}>
      <h2 id={id} className="rail__group-title">
        {title}
      </h2>
      <ul className="rail__list">{children}</ul>
    </section>
  );
}

function Accounts() {
  const accounts = useAccounts();

  return (
    <section className="rail__group rail__accounts" aria-labelledby="rail-accounts">
      <h2 id="rail-accounts" className="rail__group-title">
        Accounts
      </h2>
      {accounts.isPending && <Loading />}
      {accounts.isError && (
        <Failure error={accounts.error} retry={() => void accounts.refetch()} />
      )}
      <ul className="rail__list">
        {accounts.data?.map((account) => (
          <li key={account.id}>
            <NavLink to={`/accounts/${account.id}`} className="rail__link account-link">
              <Icon name="wallet" />
              <span className="account-link__text">
                <span className="account-link__name">{account.name}</span>
                <span className="account-link__kind">{KIND_NAMES[account.kind]}</span>
              </span>
              <span
                className={
                  isNegative(account.confirmed)
                    ? "account-link__amt account-link__amt--short"
                    : "account-link__amt"
                }
              >
                {balance(account.confirmed)}
                <span className="account-link__cur">{account.currency}</span>
              </span>
            </NavLink>
          </li>
        ))}
        <li>
          <NavLink to="/accounts/new" className="rail__link rail__link--quiet">
            <Icon name="plus" />
            <span className="rail__label">Add an account</span>
          </NavLink>
        </li>
      </ul>
    </section>
  );
}

/**
 * What is held in each currency, one line per currency.
 *
 * Never one number: a total across currencies is an estimate that moves with the rate, and
 * the server does not produce one for that reason.
 */
function Totals() {
  const totals = useTotals();
  const accounts = useAccounts();
  // With one account the total is that account's balance, already shown above it, and on
  // Home. It earns its place only when there is more than one thing to add up.
  if (!totals.data || totals.data.length === 0 || (accounts.data?.length ?? 0) < 2) return null;

  return (
    <section className="rail__group rail__totals" aria-labelledby="rail-total">
      <h2 id="rail-total" className="rail__group-title">
        Total
      </h2>
      <dl className="totals">
        {totals.data.map((total) => (
          <div key={total.currency} className="totals__row">
            <dt>{total.currency}</dt>
            <dd className={isNegative(total) ? "short" : undefined}>{balance(total)}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
