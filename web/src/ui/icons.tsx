/**
 * The menu's icons: stroked line drawings on a 24px grid at 1.8px with round caps and
 * joins, drawn to match the hairline rules (DESIGN.md, "Shapes"). Decoration beside a word,
 * never instead of one, so they are hidden from screen readers.
 */
const PATHS = {
  home: "M4 11l8-7 8 7M6 9.5V20h12V9.5M10 20v-5h4v5",
  check: "M4 6h16M4 12h10M4 18h7M15 17l2.5 2.5L22 15",
  sort: "M3.5 12.5V5a1.5 1.5 0 0 1 1.5-1.5h7.5L21 12l-8.5 8.5-9-8zM8 8h.01",
  wallet: "M4 7h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a1 1 0 0 1-1-1V7zm0 0l11-3v3M16 13.5h.01",
  plus: "M12 5v14M5 12h14",
  insights: "M4 20V4M4 20h16M8 16v-4M12 16V8M16 16v-6",
  budget: "M12 3a9 9 0 1 0 9 9M12 3v9h9M12 3a9 9 0 0 1 9 9",
  repeat: "M4 12a8 8 0 0 1 13.7-5.6L20 9M20 4v5h-5M20 12a8 8 0 0 1-13.7 5.6L4 15M4 20v-5h5",
  import: "M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19h14",
  settings: "M4 7h10M18 7h2M4 17h4M12 17h8M14 5v4M8 15v4",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name }: { name: IconName }) {
  return (
    <svg
      className="icon"
      viewBox="0 0 24 24"
      width="18"
      height="18"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
