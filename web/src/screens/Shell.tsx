import { Link, NavLink, Outlet } from "react-router";
import type { User } from "../api/types";
import { useAccounts, useInboxCount, useSignOut, useTotals } from "../api/queries";
import { KIND_NAMES, balance, isNegative } from "../ui/format";
import { Failure, Loading } from "../ui/states";
import { ThemeSwitcher } from "../ui/ThemeSwitcher";

/** The frame every signed-in screen sits in: a top bar, the accounts, and the screen. */
export function Shell({ user }: { user: User }) {
  const signOut = useSignOut();

  return (
    <div className="shell">
      <header className="topbar">
        <Link to="/" className="wordmark">
          moneytrack
        </Link>
        <div className="topbar__who">
          <ThemeSwitcher />
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

      <nav className="rail" aria-label="Accounts">
        <Accounts />
        <Totals />
        <section className="rail__section rail__more">
          <NavLink to="/" end className="rail__link">
            Home
          </NavLink>
          <InboxLink />
          <NavLink to="/breakdown" className="rail__link">
            Spending
          </NavLink>
          <NavLink to="/charts" className="rail__link">
            Charts
          </NavLink>
          <NavLink to="/outlook" className="rail__link">
            Forecast
          </NavLink>
          <NavLink to="/budgets" className="rail__link">
            Budgets
          </NavLink>
          <NavLink to="/repeating" className="rail__link">
            Regular payments
          </NavLink>
          <NavLink to="/import" className="rail__link">
            Import a statement
          </NavLink>
          <NavLink to="/categories" className="rail__link">
            Categories
          </NavLink>
        </section>
      </nav>

      <main className="main">
        <Outlet />
      </main>
    </div>
  );
}

/** The inbox, with how many lines wait in it. The count is amber because that is what amber
 * means: awaiting your check. With nothing waiting there is no count and no amber. */
function InboxLink() {
  const count = useInboxCount();
  const waiting = count.data?.total ?? 0;
  return (
    <NavLink to="/inbox" className="rail__link">
      To review
      {waiting > 0 && (
        <span className="rail__count">
          {waiting}
          <span className="visually-hidden"> waiting</span>
        </span>
      )}
    </NavLink>
  );
}

function Accounts() {
  const accounts = useAccounts();

  return (
    <section className="rail__section">
      <h2 className="mt-label rail__heading">Accounts</h2>
      {accounts.isPending && <Loading />}
      {accounts.isError && (
        <Failure error={accounts.error} retry={() => void accounts.refetch()} />
      )}
      {accounts.data && (
        <ul className="accounts">
          {accounts.data.map((account) => (
            <li key={account.id}>
              <NavLink to={`/accounts/${account.id}`} className="account-link">
                <span className="account-link__name">{account.name}</span>
                <span className="account-link__kind">{KIND_NAMES[account.kind]}</span>
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
        </ul>
      )}
      <Link to="/accounts/new" className="btn btn--ghost rail__open">
        Add an account
      </Link>
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
  if (!totals.data || totals.data.length === 0) return null;

  return (
    <section className="rail__section">
      <h2 className="mt-label rail__heading">Total</h2>
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
