import { useState, type FormEvent } from "react";
import { Link } from "react-router";
import type { CsvMapping, DateOrder, ImportFormat, ImportOutcome, ImportPreview } from "../api/types";
import { useAccounts, useImport, usePreviewImport } from "../api/queries";
import {
  MAX_STATEMENT_BYTES,
  PREVIEW_BYTES,
  columnLetter,
  fileToBase64,
  formatOf,
  guessMapping,
} from "../ui/importing";
import { Failure, FormError, Loading } from "../ui/states";
import { usePageTitle } from "../ui/title";

/** How many transactions the "as they will be read" table shows. */
const READ_ROWS = 3;

const DATE_ORDERS: { value: DateOrder; name: string }[] = [
  { value: "day_month_year", name: "Day first" },
  { value: "month_day_year", name: "Month first" },
  { value: "year_month_day", name: "Year first" },
];

/**
 * Bringing in a bank or wallet statement.
 *
 * The file goes up whole and `mt-core` reads it. A CSV needs telling once which column is
 * which; the answer is kept for that account. Nothing reaches the ledger from here: every
 * line waits in the inbox until the person has looked at it.
 */
export function ImportScreen() {
  usePageTitle("Import a statement");
  const accounts = useAccounts();
  const preview = usePreviewImport();
  const upload = useImport();

  const [accountId, setAccountId] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [format, setFormat] = useState<ImportFormat>("csv");
  const [mapping, setMapping] = useState<CsvMapping | null>(null);
  const [outcome, setOutcome] = useState<ImportOutcome | null>(null);
  const [tooLarge, setTooLarge] = useState(false);

  if (accounts.isPending) return <Loading />;
  if (accounts.isError) return <Failure error={accounts.error} retry={() => void accounts.refetch()} />;
  if (accounts.data.length === 0) {
    return (
      <section className="screen">
        <h1 className="screen__title">Import a statement</h1>
        <p className="screen__lede">
          Add an account first, so the statement has somewhere to go. <Link to="/accounts/new">Add an account</Link>
        </p>
      </section>
    );
  }
  const account = accounts.data.find((each) => each.id === accountId) ?? accounts.data[0];

  function choose(chosen: File | null, forAccount = account?.id) {
    setFile(chosen);
    setOutcome(null);
    setMapping(null);
    preview.reset();
    upload.reset();
    setTooLarge(false);
    if (!chosen || !forAccount) return;
    if (chosen.size > MAX_STATEMENT_BYTES) {
      setTooLarge(true);
      return;
    }
    const chosenFormat = formatOf(chosen.name);
    setFormat(chosenFormat);
    if (chosenFormat === "ofx") return;
    void fileToBase64(chosen, PREVIEW_BYTES).then((content) =>
      preview.mutate(
        { account_id: forAccount, content_base64: content },
        { onSuccess: (seen) => setMapping(seen.saved_mapping ?? guessMapping(seen.rows, seen.delimiter)) },
      ),
    );
  }

  function pickAccount(id: string) {
    setAccountId(id);
    // The stored mapping is per account, so a file already chosen is previewed again.
    if (file) choose(file, id);
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!file || !account) return;
    if (format === "csv" && !mapping) return;
    const content = await fileToBase64(file);
    upload.mutate(
      {
        account_id: account.id,
        file_name: file.name,
        format,
        content_base64: content,
        ...(format === "csv" && mapping ? { mapping } : {}),
      },
      { onSuccess: setOutcome },
    );
  }

  return (
    <section className="screen">
      <div className="screen__head">
        <h1 className="screen__title">Import a statement</h1>
        <p className="screen__lede">
          A CSV or OFX file downloaded from your bank or mobile money app. Its transactions wait
          in Check imports for a quick look; nothing goes into your accounts until you add it.
        </p>
      </div>

      <form className="import" onSubmit={(event) => void submit(event)} aria-label="Import a statement">
        <div className="import__pick">
          <label className="field">
            <span className="mt-label">Account</span>
            <select value={account?.id ?? ""} onChange={(event) => pickAccount(event.target.value)}>
              {accounts.data.map((each) => (
                <option key={each.id} value={each.id}>
                  {each.name} · {each.currency}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span className="mt-label">Statement file</span>
            <input
              type="file"
              accept=".csv,.txt,.ofx,.qfx,text/csv"
              onChange={(event) => choose(event.target.files?.[0] ?? null)}
            />
          </label>
        </div>

        {tooLarge && (
          <p className="error-line" role="alert">
            That file is over 5 MB. Export a shorter period and import each part.
          </p>
        )}
        {format === "ofx" && file && !tooLarge && (
          <p className="field__help">OFX files need no setup.</p>
        )}
        {format === "csv" && preview.isPending && <Loading />}
        <FormError error={preview.error} />
        {format === "csv" && preview.data && mapping && (
          <MappingForm preview={preview.data} mapping={mapping} onChange={setMapping} />
        )}

        <FormError error={upload.error} />
        <div className="entry__foot">
          <span />
          <button
            type="submit"
            className="btn btn--primary"
            disabled={!file || tooLarge || upload.isPending || (format === "csv" && !mapping)}
          >
            {upload.isPending ? "Reading the file" : format === "csv" && mapping ? "Looks right — import" : "Import"}
          </button>
        </div>
      </form>

      {outcome && <Outcome outcome={outcome} />}
    </section>
  );
}

/** Which column holds what, chosen against the first rows of the file. */
function MappingForm({
  preview,
  mapping,
  onChange,
}: {
  preview: ImportPreview;
  mapping: CsvMapping;
  onChange: (mapping: CsvMapping) => void;
}) {
  const header = mapping.has_header ? preview.rows[mapping.skip_rows] : undefined;
  const firstData = preview.rows[mapping.skip_rows + (mapping.has_header ? 1 : 0)] ?? [];
  const width = Math.max(0, ...preview.rows.map((row) => row.length));
  const columns = Array.from({ length: width }, (_, index) => {
    const name = header?.[index] || firstData[index] || "";
    return { index, name: name ? `${columnLetter(index)} · ${name}` : columnLetter(index) };
  });
  const set = (patch: Partial<CsvMapping>) => onChange({ ...mapping, ...patch });
  const amount = mapping.amount;

  const column = (label: string, value: number, pick: (index: number) => void) => (
    <label className="field">
      <span className="mt-label">{label}</span>
      <select value={value} onChange={(event) => pick(Number(event.target.value))}>
        {columns.map((each) => (
          <option key={each.index} value={each.index}>
            {each.name}
          </option>
        ))}
      </select>
    </label>
  );
  const optional = (label: string, value: number | null, pick: (index: number | null) => void) => (
    <label className="field">
      <span className="mt-label">{label}</span>
      <select
        value={value ?? ""}
        onChange={(event) => pick(event.target.value === "" ? null : Number(event.target.value))}
      >
        <option value="">Not in this file</option>
        {columns.map((each) => (
          <option key={each.index} value={each.index}>
            {each.name}
          </option>
        ))}
      </select>
    </label>
  );

  // The first few transactions as they will be read, so the person checks the result, not
  // the settings. Cells are shown as text; nothing here reads or adds an amount.
  const dataRows = preview.rows.slice(mapping.skip_rows + (mapping.has_header ? 1 : 0), mapping.skip_rows + (mapping.has_header ? 1 : 0) + READ_ROWS);
  const amountOf = (row: string[]) =>
    amount.kind === "signed"
      ? row[amount.column] || "—"
      : row[amount.money_out]
        ? `out ${row[amount.money_out]}`
        : row[amount.money_in]
          ? `in ${row[amount.money_in]}`
          : "—";

  return (
    <div className="import__mapping">
      <table className="import__read">
        <caption>How the first transactions will be read</caption>
        <thead>
          <tr>
            <th scope="col">Date</th>
            <th scope="col">Description</th>
            <th scope="col">Amount</th>
            {mapping.counterparty !== null && <th scope="col">Payee</th>}
          </tr>
        </thead>
        <tbody>
          {dataRows.map((row, index) => (
            <tr key={index}>
              <td>{row[mapping.date] || "—"}</td>
              <td>{row[mapping.label] || "—"}</td>
              <td>{amountOf(row)}</td>
              {mapping.counterparty !== null && <td>{row[mapping.counterparty] || ""}</td>}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="field__help">
        If these look right, import. If a column is wrong, fix it below; the answer is kept for
        this account.
      </p>

      <details className="import__fix">
        <summary>Fix how it's read</summary>
      <fieldset className="import__settings">
      <legend className="visually-hidden">Which column is which</legend>

      <label className="field">
        <span className="mt-label">Column headings are in</span>
        <select
          value={mapping.has_header ? String(mapping.skip_rows) : "none"}
          onChange={(event) =>
            event.target.value === "none"
              ? set({ has_header: false, skip_rows: 0 })
              : set({ has_header: true, skip_rows: Number(event.target.value) })
          }
        >
          <option value="none">No headings: the first row is a transaction</option>
          {preview.rows.slice(0, 10).map((row, index) => (
            <option key={index} value={index}>
              Row {index + 1}: {row.slice(0, 3).join(", ")}
            </option>
          ))}
        </select>
      </label>

      <div className="import__grid">
        {column("Date", mapping.date, (date) => set({ date }))}
        {column("Description", mapping.label, (label) => set({ label }))}
      </div>

      <div className="segments" role="radiogroup" aria-label="Date order">
        {DATE_ORDERS.map((order) => (
          <label key={order.value} className="segment">
            <input
              type="radio"
              name="date-order"
              checked={mapping.date_format === order.value}
              onChange={() => set({ date_format: order.value })}
            />
            <span>{order.name}</span>
          </label>
        ))}
      </div>

      <div className="segments segments--pair" role="radiogroup" aria-label="Amount columns">
        <label className="segment">
          <input
            type="radio"
            name="amount-kind"
            checked={amount.kind === "signed"}
            onChange={() =>
              set({ amount: { kind: "signed", column: amount.kind === "split" ? amount.money_out : amount.column } })
            }
          />
          <span>One amount column</span>
        </label>
        <label className="segment">
          <input
            type="radio"
            name="amount-kind"
            checked={amount.kind === "split"}
            onChange={() => {
              if (amount.kind === "split") return;
              const out = amount.column;
              set({ amount: { kind: "split", money_out: out, money_in: out + 1 < width ? out + 1 : Math.max(0, out - 1) } });
            }}
          />
          <span>Separate money in and money out columns</span>
        </label>
      </div>

      <div className="import__grid">
        {amount.kind === "signed" ? (
          column("Amount, minus for money out", amount.column, (index) =>
            set({ amount: { kind: "signed", column: index } }),
          )
        ) : (
          <>
            {column("Money out", amount.money_out, (index) => set({ amount: { ...amount, money_out: index } }))}
            {column("Money in", amount.money_in, (index) => set({ amount: { ...amount, money_in: index } }))}
          </>
        )}
        {optional("Payee", mapping.counterparty, (counterparty) => set({ counterparty }))}
        {optional("Bank reference", mapping.reference, (reference) => set({ reference }))}
        {optional("Balance after", mapping.balance, (balance) => set({ balance }))}
      </div>

      </fieldset>
      </details>
      <p className="field__help">Any line that can't be read is listed after the import.</p>
    </div>
  );
}

/** What the upload did, and where its lines went. */
function Outcome({ outcome }: { outcome: ImportOutcome }) {
  const extra = outcome.problem_count - outcome.problems.length;
  return (
    <section className="import__outcome" aria-label="What was imported" role="status">
      <p>
        Found {outcome.read} {outcome.read === 1 ? "transaction" : "transactions"}.{" "}
        {outcome.added > 0 ? (
          <>
            {outcome.added} {outcome.added === 1 ? "is" : "are"} new.{" "}
            <Link to="/inbox">Review them now</Link>
          </>
        ) : (
          "Nothing new."
        )}
        {outcome.already_seen > 0 && ` ${outcome.already_seen} ${outcome.already_seen === 1 ? "was" : "were"} already imported, so skipped.`}
      </p>
      {outcome.problem_count > 0 && (
        <>
          <p className="mt-label">
            {outcome.problem_count} {outcome.problem_count === 1 ? "line" : "lines"} couldn't be read
          </p>
          <ul className="import__problems">
            {outcome.problems.map((problem, index) => (
              <li key={`${problem.line_no}-${index}`}>
                Line {problem.line_no}: {problem.reason}
              </li>
            ))}
            {extra > 0 && <li>and {extra} more</li>}
          </ul>
        </>
      )}
    </section>
  );
}
