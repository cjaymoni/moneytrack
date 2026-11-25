import { Link } from "react-router";
import type { User } from "../api/types";
import { useSignOut } from "../api/queries";
import { HowItWorks } from "../ui/HowItWorks";
import { ThemeSwitcher } from "../ui/ThemeSwitcher";
import { usePageTitle } from "../ui/title";

/** The things set once and rarely touched: how it looks, your categories, and your sign-in. */
export function SettingsScreen({ user }: { user: User }) {
  usePageTitle("Settings");
  const signOut = useSignOut();

  return (
    <section className="screen settings">
      <h1 className="screen__title">Settings</h1>

      <section className="home__block" aria-labelledby="settings-look">
        <h2 id="settings-look" className="home__heading">
          Appearance
        </h2>
        <p className="field__help">Auto follows your device's light or dark setting.</p>
        <div className="settings__theme">
          <ThemeSwitcher />
        </div>
      </section>

      <section className="home__block" aria-labelledby="settings-categories">
        <h2 id="settings-categories" className="home__heading">
          Categories
        </h2>
        <p className="field__help">Add, rename or delete the categories your transactions are filed under.</p>
        <Link to="/categories" className="home__more">
          Manage categories
        </Link>
      </section>

      <section className="home__block" aria-labelledby="settings-help">
        <h2 id="settings-help" className="home__heading">
          Help
        </h2>
        <HowItWorks open />
      </section>

      <section className="home__block" aria-labelledby="settings-account">
        <h2 id="settings-account" className="home__heading">
          Your sign-in
        </h2>
        <p className="field__help">Signed in as {user.email}.</p>
        <button
          type="button"
          className="btn btn--ghost screen__action"
          onClick={() => signOut.mutate()}
          disabled={signOut.isPending}
        >
          Sign out
        </button>
      </section>
    </section>
  );
}
