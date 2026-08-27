# Messaging between family members

The session behavior around the family's notification repo: what a session checks at acquaint, and
what it does with a request. The protocol itself is not here. The messages repo's `README.md` is the
authority on record shape, modes, and ownership, and this file defers to it on every point it
covers.

Universal file, shared with the template repository. A proposed change is edited here and converges
at the template ([Changing the agent-files](../AGENTS.md#changing-the-agent-files)). Project-local
content goes in [custom.md](../custom.md).

## Where the repo and the record are

Both come from the work side's `[family]` table, never from prose:

- `family.messages`: the messages repo's path, relative to the config file's directory.
- `family.member`: this repo's member name, which is also its record file there, `<member>.md`.

A project that is not a family member has no `[family]` table, and nothing in this file applies to
it.

## At acquaint, check the record file

Open `<family.messages>/<family.member>.md` at every acquaint, and read it by its fields:

- a record without `read:` is unread. Reading it is what adds the field, so the sender can see the
  message arrived.
- a record without an `outcome-*` field is open traffic, whatever its age.

The persistence policy at the bottom of the record file is the file's own and governs what may be
deleted there, which by the usual policy is nothing.

## A request becomes an entry, and the reply cites it

An incoming request becomes a Todo or a backlog entry before anything is done about it, and the
reply cites that entry (wink, 2026-08-12). The reason is durability on both ends: a commit then has
an entry to reference, and the entry outlives the exchange, so the request's fate is readable from
this repo's records after the messages repo has moved on.

The reply is a record in the sender's file, written per the protocol's modes. A reply that cites a
landed commit wants the durable mode, since the permalink needs the push to exist first.
