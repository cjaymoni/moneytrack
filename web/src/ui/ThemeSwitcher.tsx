import { useId } from "react";
import { THEME_CHOICES, useTheme } from "./theme";

/** Auto, light or dark, as three segments. Auto follows the device. */
export function ThemeSwitcher() {
  const [theme, setTheme] = useTheme();
  // Its own radio group: the page can hold two switches (top bar and phone menu), and a
  // shared name would make them one group of six.
  const group = useId();
  return (
    <div className="segments segments--compact" role="radiogroup" aria-label="Theme">
      {THEME_CHOICES.map((choice) => (
        <label key={choice.value} className="segment">
          <input
            type="radio"
            name={group}
            value={choice.value}
            checked={theme === choice.value}
            onChange={() => setTheme(choice.value)}
          />
          <span>{choice.name}</span>
        </label>
      ))}
    </div>
  );
}
