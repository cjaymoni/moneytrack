import type { Category, CategoryKind, Direction } from "../api/types";

/** The kind of category a row of this direction is filed under. */
export function kindFor(direction: Direction): CategoryKind {
  return direction === "in" ? "income" : "expense";
}

/**
 * The categories of one kind in the order a person should see them: each root in its own
 * order, followed by its children.
 */
export function ordered(categories: Category[], kind: CategoryKind): Category[] {
  const ofKind = categories.filter((category) => category.kind === kind);
  const roots = ofKind.filter((category) => category.parent_id === null);
  const children = (parent: string) =>
    ofKind.filter((category) => category.parent_id === parent);
  // A child whose parent has gone (deleted, or of the other kind) still shows, at the end.
  const placed = new Set<string>();
  const out: Category[] = [];
  for (const root of roots) {
    out.push(root);
    placed.add(root.id);
    for (const child of children(root.id)) {
      out.push(child);
      placed.add(child.id);
    }
  }
  return out.concat(ofKind.filter((category) => !placed.has(category.id)));
}

/** A category's name as a chip shows it: a child carries its parent's name before it. */
export function fullName(category: Category, all: Category[]): string {
  const parent = category.parent_id && all.find((other) => other.id === category.parent_id);
  return parent ? `${parent.name} · ${category.name}` : category.name;
}

/**
 * A row of category chips. One may be chosen, or none: pressing the chosen one again
 * leaves the row uncategorised, which is a real answer rather than an error.
 */
export function CategoryPicker({
  categories,
  kind,
  value,
  onChange,
  label,
  disabled = false,
}: {
  categories: Category[];
  kind: CategoryKind;
  value: string | null;
  onChange: (categoryId: string | null) => void;
  label: string;
  disabled?: boolean;
}) {
  const choices = ordered(categories, kind);
  if (choices.length === 0) {
    return <p className="field__help">No {kind === "income" ? "income" : "spending"} categories yet.</p>;
  }
  return (
    <div className="chips" role="group" aria-label={label}>
      {choices.map((category) => {
        const chosen = category.id === value;
        return (
          <button
            key={category.id}
            type="button"
            className="chip"
            aria-pressed={chosen}
            disabled={disabled}
            onClick={() => onChange(chosen ? null : category.id)}
          >
            {fullName(category, categories)}
          </button>
        );
      })}
    </div>
  );
}
