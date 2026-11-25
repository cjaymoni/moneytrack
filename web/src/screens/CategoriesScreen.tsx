import { useState, type FormEvent } from "react";
import type { Category, CategoryKind } from "../api/types";
import { useCategories, useCreateCategory, useDeleteCategory, useRenameCategory } from "../api/queries";
import { fullName, ordered } from "../ui/CategoryPicker";
import { Failure, FormError, Loading } from "../ui/states";
import { usePageTitle } from "../ui/title";

const SIDES: { kind: CategoryKind; title: string }[] = [
  { kind: "expense", title: "Spending" },
  { kind: "income", title: "Income" },
];

/** The categories, each side in its order, each one renameable and deletable, and a form
 * to add one when asked. */
export function CategoriesScreen() {
  usePageTitle("Categories");
  const categories = useCategories();
  const [adding, setAdding] = useState(false);

  if (categories.isPending) return <Loading />;
  if (categories.isError) {
    return <Failure error={categories.error} retry={() => void categories.refetch()} />;
  }

  return (
    <section className="screen">
      <div className="screen__head">
        <h1 className="screen__title">Categories</h1>
        <p className="screen__lede">
          What each transaction was for. A category can have others inside it — Transport, with
          Trotro inside.
        </p>
      </div>

      {adding ? (
        <AddCategory categories={categories.data} onClose={() => setAdding(false)} />
      ) : (
        <button type="button" className="btn btn--primary screen__action" onClick={() => setAdding(true)}>
          Add a category
        </button>
      )}

      <div className="sides">
        {SIDES.map(({ kind, title }) => (
          <section key={kind} className="side" aria-labelledby={`side-${kind}`}>
            <h2 id={`side-${kind}`} className="mt-label side__title">
              {title}
            </h2>
            <ul className="tree">
              {ordered(categories.data, kind).map((category) => (
                <CategoryRow key={category.id} category={category} all={categories.data} />
              ))}
            </ul>
          </section>
        ))}
      </div>
    </section>
  );
}

/** One category, with renaming and deleting opened in place. */
function CategoryRow({ category, all }: { category: Category; all: Category[] }) {
  const [mode, setMode] = useState<"view" | "rename" | "delete">("view");
  const rename = useRenameCategory();
  const remove = useDeleteCategory();
  const [name, setName] = useState(category.name);
  const [moveTo, setMoveTo] = useState("");
  // Where its transactions can go: any other category on the same side.
  const targets = ordered(all, category.kind).filter((other) => other.id !== category.id);

  function submitRename(event: FormEvent) {
    event.preventDefault();
    rename.mutate({ id: category.id, name: name.trim() }, { onSuccess: () => setMode("view") });
  }

  function submitDelete(event: FormEvent) {
    event.preventDefault();
    remove.mutate({ id: category.id, moveTo: moveTo || null });
  }

  return (
    <li className={category.parent_id ? "tree__item tree__child" : "tree__item"}>
      {mode === "rename" ? (
        <form className="tree__edit" onSubmit={submitRename} aria-label={`Rename ${category.name}`}>
          <label className="field">
            <span className="visually-hidden">New name for {category.name}</span>
            <input required maxLength={64} value={name} onChange={(event) => setName(event.target.value)} autoFocus />
          </label>
          <button type="submit" className="btn btn--primary" disabled={rename.isPending}>
            {rename.isPending ? "Saving" : "Save"}
          </button>
          <button type="button" className="btn btn--quiet" onClick={() => setMode("view")}>
            Cancel
          </button>
          <FormError error={rename.error} />
        </form>
      ) : (
        <div className="tree__line">
          <span className="tree__name">{category.name}</span>
          <span className="tree__actions">
            <button type="button" className="btn btn--quiet" onClick={() => setMode("rename")}>
              Rename <span className="visually-hidden">{category.name}</span>
            </button>
            <button type="button" className="btn btn--quiet" onClick={() => setMode(mode === "delete" ? "view" : "delete")}>
              Delete <span className="visually-hidden">{category.name}</span>
            </button>
          </span>
        </div>
      )}
      {mode === "delete" && (
        <form className="tree__delete" onSubmit={submitDelete} aria-label={`Delete ${category.name}`}>
          <label className="field">
            <span className="mt-label">Move its transactions to</span>
            <select value={moveTo} onChange={(event) => setMoveTo(event.target.value)}>
              <option value="">No category</option>
              {targets.map((target) => (
                <option key={target.id} value={target.id}>
                  {fullName(target, all)}
                </option>
              ))}
            </select>
          </label>
          <p className="field__help">
            Its regular payments and saved payee rules move the same way. This can't be undone.
          </p>
          <div className="tree__delete-actions">
            <button type="button" className="btn btn--quiet" onClick={() => setMode("view")}>
              Keep it
            </button>
            <button type="submit" className="btn btn--grade" disabled={remove.isPending}>
              {remove.isPending ? "Deleting" : `Delete ${category.name}`}
            </button>
          </div>
          <FormError error={remove.error} />
        </form>
      )}
    </li>
  );
}

function AddCategory({ categories, onClose }: { categories: Category[]; onClose: () => void }) {
  const create = useCreateCategory();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<CategoryKind>("expense");
  const [parent, setParent] = useState("");

  // Only a root of the same kind can hold a new category: the server refuses anything else.
  const parents = categories.filter(
    (category) => category.kind === kind && category.parent_id === null,
  );

  function pickKind(value: CategoryKind) {
    setKind(value);
    setParent("");
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    create.mutate(
      { name: name.trim(), kind, ...(parent ? { parent_id: parent } : {}) },
      {
        onSuccess: () => {
          setName("");
          setParent("");
        },
      },
    );
  }

  return (
    <form className="add-category" onSubmit={submit} aria-label="Add a category">
      <div className="segments segments--pair" role="radiogroup" aria-label="Side">
        {SIDES.map(({ kind: option, title }) => (
          <label key={option} className="segment">
            <input
              type="radio"
              name="category-kind"
              value={option}
              checked={kind === option}
              onChange={() => pickKind(option)}
            />
            <span>{title}</span>
          </label>
        ))}
      </div>
      <div className="add-category__fields">
        <label className="field">
          <span className="mt-label">Name</span>
          <input
            required
            maxLength={64}
            placeholder={kind === "expense" ? "Trotro" : "Side hustle"}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <label className="field">
          <span className="mt-label">Under</span>
          <select value={parent} onChange={(event) => setParent(event.target.value)}>
            <option value="">Nothing — its own heading</option>
            {parents.map((option) => (
              <option key={option.id} value={option.id}>
                {option.name}
              </option>
            ))}
          </select>
        </label>
        <button type="submit" className="btn btn--primary" disabled={create.isPending}>
          {create.isPending ? "Adding" : "Add"}
        </button>
        <button type="button" className="btn btn--quiet" onClick={onClose}>
          Done
        </button>
      </div>
      <FormError error={create.error} />
    </form>
  );
}
