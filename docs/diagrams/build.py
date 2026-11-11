#!/usr/bin/env python3
"""Generates the schema diagram pages from the .mmd sources.

The three .mmd files are the only copy of the diagrams. This script inlines them into
docs/SCHEMA-DIAGRAM.md (which GitHub renders) and docs/diagrams/index.html (which any
browser renders, offline apart from the Mermaid script itself).

    python3 docs/diagrams/build.py
"""

from pathlib import Path

HERE = Path(__file__).resolve().parent
DOCS = HERE.parent

DIAGRAMS = [
    (
        "ledger",
        "The ledger",
        "Seven tables that exist in both databases and sync between them, plus the reporting "
        "rates. This is the part of the schema the product is actually about.",
    ),
    (
        "device",
        "On the device only",
        "The review inbox and the SMS machinery. None of it syncs: only a confirmed "
        "transaction is allowed to leave the handset.",
    ),
    (
        "server",
        "On the server only",
        "Identity, rule packs, and the columns every synced row carries. `ledger_tables` "
        "stands in for all seven, which share the same ownership and sync columns.",
    ),
]

HTML_TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>moneytrack schema</title>
<script src="https://cdn.jsdelivr.net/npm/mermaid@11.4.1/dist/mermaid.min.js"></script>
<style>
  :root {{
    --bg: #fbfaf8;
    --panel: #ffffff;
    --ink: #1c1b19;
    --muted: #6b6760;
    --rule: #e3dfd8;
  }}
  @media (prefers-color-scheme: dark) {{
    :root:not([data-theme="light"]) {{
      --bg: #16151a;
      --panel: #1e1d23;
      --ink: #f2efe9;
      --muted: #a5a099;
      --rule: #32303a;
    }}
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0;
    padding: 48px 16px 96px;
    background: var(--bg);
    color: var(--ink);
    font: 16px/1.6 ui-sans-serif, -apple-system, "Segoe UI", system-ui, sans-serif;
  }}
  main {{ max-width: 1180px; margin: 0 auto; }}
  h1 {{ font-size: 1.9rem; margin: 0 0 4px; letter-spacing: -0.02em; }}
  .lede {{ color: var(--muted); margin: 0 0 40px; max-width: 62ch; }}
  section {{
    background: var(--panel);
    border: 1px solid var(--rule);
    border-radius: 12px;
    padding: 24px;
    margin-bottom: 28px;
    overflow-x: auto;
  }}
  h2 {{ font-size: 1.15rem; margin: 0 0 4px; }}
  .note {{ color: var(--muted); margin: 0 0 20px; max-width: 70ch; font-size: 0.95rem; }}
  .mermaid {{ text-align: center; }}
  footer {{ color: var(--muted); font-size: 0.9rem; max-width: 70ch; margin: 0 auto; }}
  code {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.92em; }}
</style>
</head>
<body>
<main>
  <h1>moneytrack schema</h1>
  <p class="lede">Revision 1. Generated from <code>docs/diagrams/*.mmd</code> by
     <code>docs/diagrams/build.py</code> &mdash; edit the <code>.mmd</code> files, not this page.
     Written up in <code>docs/SCHEMA.md</code>.</p>
{sections}
  <footer>Every synced table also carries <code>created_at</code>, <code>updated_at</code>,
  <code>deleted_at</code> and <code>device_id</code>; the Postgres copies carry
  <code>user_id</code> and <code>server_seq</code> as well. Those are left off the diagrams so
  the shape stays readable.</footer>
</main>
<script>
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  mermaid.initialize({{
    startOnLoad: true,
    theme: dark ? "dark" : "neutral",
    er: {{ layoutDirection: "TB", entityPadding: 14, useMaxWidth: true }},
  }});
</script>
</body>
</html>
"""

MD_HEADER = """# Schema diagrams

Revision 1. Generated from `docs/diagrams/*.mmd` by `docs/diagrams/build.py` — edit the
`.mmd` files, not this page. The tables are described in [SCHEMA.md](SCHEMA.md).

GitHub renders the diagrams below. For a local render, open `docs/diagrams/index.html` in a
browser.

Every synced table also carries `created_at`, `updated_at`, `deleted_at` and `device_id`; the
Postgres copies carry `user_id` and `server_seq` as well. Those are left off the diagrams so
the shape stays readable.
"""


def main() -> None:
    sections_html = []
    md_parts = [MD_HEADER]

    for slug, title, note in DIAGRAMS:
        source = (HERE / f"{slug}.mmd").read_text().strip()
        sections_html.append(
            f'  <section id="{slug}">\n'
            f"    <h2>{title}</h2>\n"
            f'    <p class="note">{note}</p>\n'
            f'    <pre class="mermaid">\n{source}\n    </pre>\n'
            f"  </section>\n"
        )
        md_parts.append(f"\n## {title}\n\n{note}\n\n```mermaid\n{source}\n```\n")

    (HERE / "index.html").write_text(
        HTML_TEMPLATE.format(sections="".join(sections_html))
    )
    (DOCS / "SCHEMA-DIAGRAM.md").write_text("".join(md_parts))
    print("wrote docs/SCHEMA-DIAGRAM.md and docs/diagrams/index.html")


if __name__ == "__main__":
    main()
