# 0003 — An inbox on the server, for uploaded statements only

**Date:** 2026-11-23
**Status:** accepted
**Amends:** "Device only, never synced: `inbox_items`" in [SCHEMA.md](../SCHEMA.md), and rule 4
at the head of `db/postgres/0001_init.sql`

## The decision

The server gets an `inbox_items` table (`db/postgres/0002_import.sql`). It holds the lines of
a CSV or OFX statement a person uploaded through the browser, each with the category
`mt-core` proposed, until the person accepts, merges or rejects it.

It accepts those two sources and no others. A `CHECK` on the column refuses `sms` and
`receipt`, and `crates/core/tests/schema_agreement.rs` fails if that list ever widens. Like the
device's inbox, it has no column a message body or a raw statement line would fit in: the
parsed fields are kept, the text they were read from is not.

The device's own `inbox_items` is unchanged and still never syncs.

## Why

**The inbox was device-only because of SMS, and SMS has not moved.** Google grants the
SMS-based money management exception on the condition that parsing happens on the handset and
only a parsed transaction ever reaches a server: no message text, no unmatched message, no
list of senders. A server-side inbox for SMS would put unreviewed, parsed message content on
the server. That is still forbidden, and the `CHECK` keeps it that way structurally.

**A statement is a different thing.** The person chose a file and sent it to us, and the
browser has no database of its own to hold the lines while they are checked — that was settled
in [0002-web-first.md](0002-web-first.md). Holding the lines on the server is the only way a
review can survive closing the tab, which matters for a year of card statement read over two
evenings rather than one.

**Two alternatives were weighed.** Reviewing in the browser with nothing stored — upload,
show, commit the accepted rows in one request — needed no schema change, but lost the work
whenever the tab closed and could not remember that a rejected line was rejected. Naming the
table something other than `inbox_items` to get past the guard test would have kept the
letter of the old rule and dropped its point.

## What it costs

- A second table to keep honest. The device and server inboxes share their vocabulary and
  their status rules but not their columns: the server's has `user_id`, `import_id` and
  `line_no`, and no `device_id`, because its rows never sync.
- Uploaded statements now pass through, and their parsed lines rest on, a server we host. That
  is more of someone's financial history in our care, which phase 5's deletion and export must
  cover: a person deleting their account deletes their inbox (it cascades from `users`), and
  an export should include what is still waiting.
- The fingerprint that stops a statement being offered twice is kept for settled lines too,
  so a rejected line stays rejected. It is a digest of the parsed fields, not the text, but it
  is kept indefinitely; if that ever needs a limit, it goes on settled rows only.
