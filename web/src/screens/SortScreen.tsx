import { useState } from "react";
import { Link } from "react-router";
import type { Account, Category } from "../api/types";
import { useAccounts, useAllTransactions, useCategories, useFileMany } from "../api/queries";
import { CategoryPicker, fullName, kindFor } from "../ui/CategoryPicker";
import { gutterDate, plural, signed } from "../ui/format";
import { groupByPayee, type PayeeGroup } from "../ui/sorting";
import { Failure, FormError, Loading } from "../ui/states";
import { usePageTitle } from "../ui/title";

/** How many of a group's rows are listed before "and N more". */
const ROWS_SHOWN = 4;
/** How many categories each group shows before "More categories". */
const CHIPS_AT_FIRST = 7;

/** What was just filed, kept so it can be put back. */
interface Filed {
  payee: string;
  category: string;
  ids: string[];
}

/**
 * Every transaction without a category, grouped by who it was with, so a payee seen ten
 * times is filed with one click instead of ten — and, if the person wants, every statement
 * from that payee after this one is filed the same way.
 */
export function SortScreen() {
  usePageTitle("Sort into categories");
  const transactions = useAllTransactions();
  const categories = useCategories();
  const accounts = useAccounts();
  const file = useFileMany();
  const [last, setLast] = useState<Filed | null>(null);

  if (transactions.isPending || categories.isPending) return <Loading />;
  if (transactions.isError) {
    return <Failure error={transactions.error} retry={() => void transactions.refetch()} />;
  }
  if (categories.isError) {
    return <Failure error={categories.error} retry={() => void categories.refetch()} />;
  }

  const groups = groupByPayee(transactions.data);
  const undo = () => {
    if (!last) return;
    file.mutate({ transactionIds: last.ids, categoryId: null }, { onSuccess: () => setLast(null) });
  };

  return (
    <section className="screen sort">
      <header className="screen__head">
        <h1 className="screen__title">Sort into categories</h1>
        <p className="screen__lede">
          Transactions with no category, grouped by who they were with. Pick a category to file
          the whole group at once.
        </p>
      </header>

      <div className="live" role="status">
        {last && (
          <p className="sort__done">
            <span>
              Filed {last.ids.length} from <b>{last.payee}</b> under {last.category}.
            </span>
            <button type="button" className="btn btn--quiet" onClick={undo} disabled={file.isPending}>
              Undo
            </button>
          </p>
        )}
      </div>
      <FormError error={file.error} />

      {groups.length === 0 ? (
        <p className="field__help">
          Everything has a category. <Link to="/">Back to Home</Link>
        </p>
      ) : (
        <ol className="sort__groups" aria-label="Payees to sort">
          {groups.map((group) => (
            <PayeeCard
              key={group.key}
              group={group}
              categories={categories.data}
              accounts={accounts.data ?? []}
              busy={file.isPending}
              onFile={(categoryId, remember) => {
                const chosen = categories.data.find((category) => category.id === categoryId);
                const ids = group.rows.map((row) => row.id);
                file.mutate(
                  {
                    transactionIds: ids,
                    categoryId,
                    ...(remember ? { remember: { matched_field: group.field, pattern: group.payee } } : {}),
                  },
                  {
                    onSuccess: () =>
                      setLast({
                        payee: group.payee,
                        category: chosen ? fullName(chosen, categories.data) : "that category",
                        ids,
                      }),
                  },
                );
              }}
            />
          ))}
        </ol>
      )}
    </section>
  );
}

interface PayeeCardProps {
  group: PayeeGroup;
  categories: Category[];
  accounts: Account[];
  busy: boolean;
  onFile: (categoryId: string, remember: boolean) => void;
}

function PayeeCard({ group, categories, accounts, busy, onFile }: PayeeCardProps) {
  const [remember, setRemember] = useState(true);
  const headingId = `payee-${group.key.replace(/[^a-z0-9]/gi, "-")}`;
  const accountName = (id: string) => accounts.find((account) => account.id === id)?.name;
  const shown = group.rows.slice(-ROWS_SHOWN).reverse();
  const hidden = group.rows.length - shown.length;
  const count = group.rows.length;

  return (
    <li className="sort__group" aria-labelledby={headingId}>
      <div className="sort__head">
        <h2 id={headingId} className="sort__payee">
          {group.payee}
        </h2>
        <span className="home__muted">
          {count} {plural(count, { one: "transaction", other: "transactions" })} ·{" "}
          {group.direction === "out" ? "money out" : "money in"}
        </span>
      </div>
      <ul className="sort__rows">
        {shown.map((row) => (
          <li key={row.id}>
            <span className="home__muted">{gutterDate(row.booked_at)}</span> {row.label}
            {accountName(row.account_id) && <span className="home__muted"> · {accountName(row.account_id)}</span>}
            <span className={row.direction === "in" ? "sort__amt in" : "sort__amt"}>
              {signed(row.amount, row.direction)}
            </span>
          </li>
        ))}
        {hidden > 0 && <li className="home__muted">and {hidden} earlier</li>}
      </ul>
      <CategoryPicker
        categories={categories}
        kind={kindFor(group.direction)}
        value={null}
        onChange={(categoryId) => categoryId && onFile(categoryId, remember)}
        label={`Category for ${group.payee}`}
        disabled={busy}
        shown={CHIPS_AT_FIRST}
      />
      <label className="check">
        <input type="checkbox" checked={remember} onChange={(event) => setRemember(event.target.checked)} />
        <span>File future transactions from {group.payee} the same way</span>
      </label>
    </li>
  );
}
