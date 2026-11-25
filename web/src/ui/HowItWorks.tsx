/**
 * The handful of ideas the app rests on, in a sentence each, for someone new. Shown in
 * Settings and on Home while getting started.
 */
export function HowItWorks({ open = false }: { open?: boolean }) {
  return (
    <details className="how" open={open}>
      <summary>How moneytrack works</summary>
      <dl className="how__list">
        <div>
          <dt>Accounts</dt>
          <dd>Each place you keep money: your MoMo wallet, a bank account, the cash in your pocket.</dd>
        </div>
        <div>
          <dt>Transactions</dt>
          <dd>Money in or out of an account. Add them by hand, or import a statement from your bank or MoMo app.</dd>
        </div>
        <div>
          <dt>Categories</dt>
          <dd>What a transaction was for, like Transport or Rent. They make Insights and Budgets work.</dd>
        </div>
        <div>
          <dt>Planned</dt>
          <dd>Something that hasn't happened yet, like next month's rent. It shows in the Forecast until you mark it paid.</dd>
        </div>
        <div>
          <dt>Imported</dt>
          <dd>Came from a statement. Imported transactions wait in Check imports until you've looked at them.</dd>
        </div>
        <div>
          <dt>Adjustment</dt>
          <dd>Added when you correct an account's balance to match what's really there.</dd>
        </div>
      </dl>
    </details>
  );
}
