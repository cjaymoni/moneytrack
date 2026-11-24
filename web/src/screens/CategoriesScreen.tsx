import { useState, type FormEvent } from "react";
import type { Category, CategoryKind } from "../api/types";
import { useCategories, useCreateCategory } from "../api/queries";
import { ordered } from "../ui/CategoryPicker";
import { Failure, FormError, Loading } from "../ui/states";

const SIDES: { kind: CategoryKind; title: string }[] = [
  { kind: "expense", title: "Spending" },
  { kind: "income", title: "Income" },
];

/** The categories, each side in its order, and a line to add one. */
export function CategoriesScreen() {
  const categories = useCategories();

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

      <AddCategory categories={categories.data} />

      <div className="sides">
        {SIDES.map(({ kind, title }) => (
          <section key={kind} className="side" aria-labelledby={`side-${kind}`}>
            <h2 id={`side-${kind}`} className="mt-label side__title">
              {title}
            </h2>
            <ul className="tree">
              {ordered(categories.data, kind).map((category) => (
                <li key={category.id} className={category.parent_id ? "tree__child" : undefined}>
                  {category.name}
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </section>
  );
}

function AddCategory({ categories }: { categories: Category[] }) {
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
      </div>
      <FormError error={create.error} />
    </form>
  );
}
