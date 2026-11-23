import { useSearchParams } from "react-router";
import type { Outlook, OutlookPoint } from "../api/types";
import { useOutlook } from "../api/queries";
import { KIND_NAMES, balance, gutterDate, isNegative, longDate, signed } from "../ui/format";
import { Failure, Loading } from "../ui/states";

const WINDOWS = [30, 90, 180] as const;

/**
 * Where each account is headed, and the first day it runs short.
 *
 * One section per account and never a pooled total: money does not move between accounts by
 * itself, so a surplus in the bank does not cover a shortfall in the wallet until someone
 * moves it.
 *
 * The line is drawn from levels `mt-core` worked out. Across, it is placed by time, which is
 * this screen's to measure; up and down, only by the level the server sent.
 */
export function OutlookScreen() {
  const [params, setParams] = useSearchParams();
  const asked = Number(params.get("days"));
  const days = (WINDOWS as readonly number[]).includes(asked) ? asked : 90;
  const outlook = useOutlook(days);

  return (
    <section className="screen">
      <header className="month-bar">
        <h1 className="screen__title">Outlook</h1>
        <div className="segments window-pick" role="radiogroup" aria-label="How far ahead">
          {WINDOWS.map((option) => (
            <label key={option} className="segment">
              <input
                type="radio"
                name="outlook-days"
                checked={option === days}
                onChange={() => setParams(option === 90 ? {} : { days: String(option) })}
              />
              <span>{option} days</span>
            </label>
          ))}
        </div>
      </header>

      {outlook.isPending && <Loading />}
      {outlook.isError && <Failure error={outlook.error} retry={() => void outlook.refetch()} />}
      {outlook.data && outlook.data.length === 0 && (
        <p className="screen__lede">Open an account to see where it is headed.</p>
      )}
      {outlook.data?.map((each) => (
        <AccountOutlook key={each.account.id} outlook={each} days={days} />
      ))}
      {outlook.data && outlook.data.length > 0 && (
        <p className="field__help">
          Counted from what each account holds now and every expected row ahead. Accounts are
          not added together: money in one does not cover a shortfall in another until you
          move it.
        </p>
      )}
    </section>
  );
}

function AccountOutlook({ outlook, days }: { outlook: Outlook; days: number }) {
  const { account, points } = outlook;
  const short = points.find((point) => point.transaction.id === outlook.shortfall_transaction_id);
  const lowRow = points.find((point) => point.transaction.id === outlook.low_transaction_id);

  return (
    <section className="outlook" aria-labelledby={`outlook-${account.id}`}>
      <h2 id={`outlook-${account.id}`} className="mt-label outlook__name">
        {account.name} <span className="balance__kind">· {KIND_NAMES[account.kind]}</span>
      </h2>

      <Verdict outlook={outlook} short={short} days={days} />

      <div className="balance">
        <p className="mt-label">{lowRow ? "Lowest ahead" : "Lowest ahead — today"}</p>
        <p className="balance__fig">
          <span className={isNegative(outlook.low) ? "short" : undefined}>{balance(outlook.low)}</span>
          <span className="balance__cur">{account.currency}</span>
        </p>
        <p className="mt-label balance__meta">
          {lowRow ? (
            <>
              On {gutterDate(lowRow.transaction.booked_at)}, after {lowRow.transaction.label} ·
              holds <span className="balance__then">{balance(outlook.start)}</span> now
            </>
          ) : (
            <>
              Holds <span className="balance__then">{balance(outlook.start)}</span> now
            </>
          )}
        </p>
      </div>

      <Line outlook={outlook} />

      {points.length === 0 ? (
        <p className="field__help">Nothing expected in the next {days} days.</p>
      ) : (
        <ol className="ledger__rows outlook__rows" aria-label={`Expected in ${account.name}`}>
          {points.map((point) => (
            <Ahead
              key={point.transaction.id}
              point={point}
              short={point.transaction.id === outlook.shortfall_transaction_id}
            />
          ))}
        </ol>
      )}
    </section>
  );
}

/** The one sentence that matters: does it run short, and when. */
function Verdict({ outlook, short, days }: { outlook: Outlook; short: OutlookPoint | undefined; days: number }) {
  if (outlook.shortfall === "already") {
    return (
      <p className="verdict verdict--short" role="alert">
        Already short: it holds {balance(outlook.start)} {outlook.account.currency} now. An
        expected row dated in the past may need confirming or removing.
      </p>
    );
  }
  if (outlook.shortfall === "at" && short) {
    return (
      <p className="verdict verdict--short" role="alert">
        Runs short on {longDate(short.transaction.booked_at)}: {short.transaction.label} takes it
        to {balance(short.balance_after)} {outlook.account.currency}.
      </p>
    );
  }
  return <p className="verdict">Stays above zero for the next {days} days.</p>;
}

/**
 * The balance as a stepped line: flat until a row lands, then straight to where it leaves
 * the account. Decoration for sighted readers; the list beneath says everything it says.
 */
function Line({ outlook }: { outlook: Outlook }) {
  const from = Date.parse(outlook.from);
  const until = Date.parse(outlook.until);
  const span = Math.max(until - from, 1);
  // Across by time — this screen's to measure. Up by level — the server's.
  const x = (instant: string) => Math.min(Math.max((Date.parse(instant) - from) / span, 0), 1) * 1000;
  const y = (level: number) => 190 - (level / 10_000) * 180;

  let path = `M 0 ${y(outlook.start_level_bps)}`;
  let level = outlook.start_level_bps;
  for (const point of outlook.points) {
    const across = x(point.transaction.booked_at);
    path += ` H ${across} V ${y(point.level_bps)}`;
    level = point.level_bps;
  }
  path += ` H 1000 V ${y(level)}`;

  const low = outlook.points.find((point) => point.transaction.id === outlook.low_transaction_id);
  const lowLeft = low ? x(low.transaction.booked_at) / 10 : 0;
  const lowTop = (y(low ? low.level_bps : outlook.start_level_bps) / 200) * 100;

  return (
    <div className="line" aria-hidden="true">
      <svg viewBox="0 0 1000 200" preserveAspectRatio="none" className="line__svg">
        <line
          x1="0"
          x2="1000"
          y1={y(outlook.zero_level_bps)}
          y2={y(outlook.zero_level_bps)}
          className="line__zero"
          vectorEffect="non-scaling-stroke"
        />
        <path d={path} className="line__path" vectorEffect="non-scaling-stroke" />
      </svg>
      <span className="line__zero-label mt-micro" style={{ top: `${(y(outlook.zero_level_bps) / 200) * 100}%` }}>
        0
      </span>
      <span className="line__low" style={{ left: `${lowLeft}%`, top: `${lowTop}%` }} />
      <span className="line__ends mt-micro">
        <span>Today</span>
        <span>{gutterDate(outlook.until)}</span>
      </span>
    </div>
  );
}

function Ahead({ point, short }: { point: OutlookPoint; short: boolean }) {
  const { transaction, balance_after } = point;
  return (
    <li className={short ? "mt-row mt-row--future outlook__short" : "mt-row mt-row--future"}>
      <time className="date" dateTime={transaction.booked_at} title={longDate(transaction.booked_at)}>
        {gutterDate(transaction.booked_at)}
      </time>
      <div>
        <div className="label">{transaction.label}</div>
        {short && <div className="sub outlook__flag">Runs short here</div>}
      </div>
      <div className="figures">
        <div className={transaction.direction === "in" ? "amt in" : "amt"}>
          {signed(transaction.amount, transaction.direction)}
        </div>
        <div className={isNegative(balance_after) ? "after short" : "after"}>
          <span className="visually-hidden">then held </span>
          {balance(balance_after)}
        </div>
      </div>
    </li>
  );
}
