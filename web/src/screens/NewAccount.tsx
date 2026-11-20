import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import type { AccountKind } from "../api/types";
import { useCreateAccount, useCurrencies } from "../api/queries";
import { KIND_NAMES } from "../ui/format";
import { FormError } from "../ui/states";

const KINDS: AccountKind[] = ["momo", "bank", "cash", "card"];

/** Opening an account: what it is called, what kind, which currency, what it holds now. */
export function NewAccount() {
  const navigate = useNavigate();
  const create = useCreateAccount();
  const currencies = useCurrencies();

  const [name, setName] = useState("");
  const [kind, setKind] = useState<AccountKind>("momo");
  const [currency, setCurrency] = useState("GHS");
  const [opening, setOpening] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = opening.trim();
    create.mutate(
      {
        name: name.trim(),
        kind,
        currency,
        ...(trimmed ? { opening_balance: trimmed } : {}),
      },
      { onSuccess: (account) => void navigate(`/accounts/${account.id}`) },
    );
  }

  return (
    <section className="screen screen--narrow">
      <h1 className="screen__title">Open an account</h1>
      <p className="screen__lede">
        A wallet, a bank account, the cash in your pocket. Each holds one currency.
      </p>

      <form className="form" onSubmit={submit}>
        <label className="field">
          <span className="mt-label">Name</span>
          <input
            required
            maxLength={64}
            placeholder="MTN MoMo"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>

        <fieldset className="field">
          <legend className="mt-label">Kind</legend>
          <div className="segments" role="radiogroup">
            {KINDS.map((option) => (
              <label key={option} className="segment">
                <input
                  type="radio"
                  name="kind"
                  value={option}
                  checked={kind === option}
                  onChange={() => setKind(option)}
                />
                <span>{KIND_NAMES[option]}</span>
              </label>
            ))}
          </div>
        </fieldset>

        <label className="field">
          <span className="mt-label">Currency</span>
          <select value={currency} onChange={(event) => setCurrency(event.target.value)}>
            {(currencies.data ?? [{ code: "GHS", name: "Ghanaian cedi" }]).map((option) => (
              <option key={option.code} value={option.code}>
                {option.code} — {option.name}
              </option>
            ))}
          </select>
        </label>

        <label className="field">
          <span className="mt-label">What it holds today</span>
          <input
            inputMode="decimal"
            autoComplete="off"
            placeholder="0"
            className="amount-input"
            value={opening}
            onChange={(event) => setOpening(event.target.value)}
          />
          <span className="field__help">
            In {currency}, as you would write it — 250.50. Leave it empty for nothing.
          </span>
        </label>

        <FormError error={create.error} />

        <button type="submit" className="btn btn--primary" disabled={create.isPending}>
          {create.isPending ? "Opening" : "Open account"}
        </button>
      </form>
    </section>
  );
}
