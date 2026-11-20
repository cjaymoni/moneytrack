import { useParams } from "react-router";
import { ApiError } from "../api/client";
import { useLedger } from "../api/queries";
import { KIND_NAMES, balance, isNegative } from "../ui/format";
import { Failure, Loading } from "../ui/states";
import { EntryForm } from "./EntryForm";
import { LedgerList } from "./LedgerList";

/** One account: what it holds, a line to record a row, and the ledger through today. */
export function AccountScreen() {
  const { accountId = "" } = useParams();
  const ledger = useLedger(accountId);

  if (ledger.isPending) return <Loading />;
  if (ledger.isError) {
    if (ledger.error instanceof ApiError && ledger.error.code === "not_found") {
      return <Failure error={new ApiError(404, "not_found", "There is no such account.")} />;
    }
    return <Failure error={ledger.error} retry={() => void ledger.refetch()} />;
  }

  const { account, entries } = ledger.data;
  const forecastDiffers = account.projected.minor_units !== account.confirmed.minor_units;

  return (
    <section className="screen">
      <header className="balance">
        <p className="mt-label balance__name">
          {account.name} <span className="balance__kind">· {KIND_NAMES[account.kind]}</span>
        </p>
        <p className="balance__fig">
          <span className={isNegative(account.confirmed) ? "short" : undefined}>
            {balance(account.confirmed)}
          </span>
          <span className="balance__cur">{account.currency}</span>
        </p>
        <p className="mt-label balance__meta">
          {forecastDiffers ? (
            <>
              Held now · <span className="balance__then">{balance(account.projected)}</span> once
              expected rows land
            </>
          ) : (
            "Held now"
          )}
        </p>
      </header>

      <EntryForm account={account} />

      <LedgerList entries={entries} opening={ledger.data.opening} currency={account.currency} />
    </section>
  );
}
