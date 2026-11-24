import { useState, type FormEvent } from "react";
import { Link } from "react-router";
import { useSignIn } from "../api/queries";
import { FormError } from "../ui/states";
import { ThemeSwitcher } from "../ui/ThemeSwitcher";

/** Signing in and registering: one form, two verbs. */
export function SignIn({ mode }: { mode: "login" | "register" }) {
  const signIn = useSignIn(mode);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const registering = mode === "register";

  function submit(event: FormEvent) {
    event.preventDefault();
    signIn.mutate({ email, password });
  }

  return (
    <main className="gate">
      <div className="gate__inner">
        <p className="wordmark">moneytrack</p>
        <h1 className="gate__title">{registering ? "Sign up" : "Sign in"}</h1>
        <p className="gate__lede">
          {registering
            ? "Track every cedi in and out, across MoMo, bank and cash."
            : "Welcome back."}
        </p>

        <form className="form" onSubmit={submit} noValidate={false}>
          <label className="field">
            <span className="mt-label">Email</span>
            <input
              type="email"
              autoComplete="email"
              required
              value={email}
              onChange={(event) => setEmail(event.target.value)}
            />
          </label>
          <label className="field">
            <span className="mt-label">Password</span>
            <input
              type="password"
              autoComplete={registering ? "new-password" : "current-password"}
              required
              minLength={registering ? 12 : undefined}
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
            {registering && <span className="field__help">At least 12 characters.</span>}
          </label>

          <FormError error={signIn.error} />

          <button type="submit" className="btn btn--primary" disabled={signIn.isPending}>
            {signIn.isPending ? "One moment" : registering ? "Sign up" : "Sign in"}
          </button>
        </form>

        <p className="gate__switch">
          {registering ? (
            <>
              Already signed up? <Link to="/signin">Sign in</Link>
            </>
          ) : (
            <>
              New here? <Link to="/register">Sign up</Link>
            </>
          )}
        </p>

        <div className="gate__theme">
          <ThemeSwitcher />
        </div>
      </div>
    </main>
  );
}
