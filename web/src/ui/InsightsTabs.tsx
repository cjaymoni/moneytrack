import { NavLink } from "react-router";

/** The three ways of looking at your money, as tabs across the top of each. */
const TABS = [
  { to: "/breakdown", name: "Spending", hint: "One month by category" },
  { to: "/charts", name: "Trends", hint: "Month by month" },
  { to: "/outlook", name: "Forecast", hint: "The weeks ahead" },
] as const;

/**
 * Spending, Trends and Forecast are one menu item, Insights; these tabs move between them
 * and say which one you are on.
 */
export function InsightsTabs() {
  return (
    <nav className="tabs" aria-label="Insights">
      {TABS.map((tab) => (
        <NavLink key={tab.to} to={tab.to} className="tabs__tab">
          <span className="tabs__name">{tab.name}</span>
          <span className="tabs__hint">{tab.hint}</span>
        </NavLink>
      ))}
    </nav>
  );
}
