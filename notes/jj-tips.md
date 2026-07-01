# Jujutsu (jj) Tips

See [Steve Klabnik](https://github.com/steveklabnik)
[Jujutsu-tutorial](https://steveklabnik.github.io/jujutsu-tutorial)
and [jj docs](https://docs.jj-vcs.dev/latest/).

## Initial Commit for a repo

Create a directory and add your files.

Minimal commands to push

```
jj git init .
jj describe
jj git remote add origin git@github.com:winksaville/vc-template-x1
jj bookmark create main -r @
jj bookmark track main --remote=origin
jj git push
```

## Push a change to main

Assuming that this is to be push to main you
set the bookmark to the appropriate commit and
then just push:

```
jj bookmark set main -r @
jj git push
```

Complete example:
```
wink@3900x 26-03-13T17:26:21.177Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
$ vi README.md 
wink@3900x 26-03-13T17:28:08.833Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
$ jj log
@  vnsyoswv wink@saville.com 2026-03-13 10:28:15 main* 3ac24f49
│  feat: Update README.md
◆  vuwzvmwm wink@saville.com 2026-03-13 09:38:22 main@origin 1a79f803
│  feat: Initial commit for the vibe coding main repo
~
wink@3900x 26-03-13T17:28:15.704Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
$ jj git push
Changes to push to origin:
  Move forward bookmark main from 1a79f803025f to 3ac24f49321b
git: Enumerating objects: 5, done.
git: Counting objects: 100% (5/5), done.
git: Delta compression using up to 24 threads
git: Compressing objects: 100% (3/3), done.
git: Writing objects: 100% (3/3), 790 bytes | 790.00 KiB/s, done.
git: Total 3 (delta 2), reused 0 (delta 0), pack-reused 0 (from 0)
remote: Resolving deltas: 100% (2/2), completed with 2 local objects.
Warning: The working-copy commit in workspace 'default' became immutable, so a new commit has been created on top of it.
Working copy  (@) now at: kywoutls c26d415e (empty) (no description set)
Parent commit (@-)      : vnsyoswv 3ac24f49 main | feat: Update README.md
wink@3900x 26-03-13T17:28:33.741Z:~/data/prgs/rust/vc-template-x1 ((main))
```

## Example of modifying an existing commit and "force" push

Tweak a commit and push it using `jj edit` then "force" push:

Minimum steps changing xx but it could be any commit on main
or other bookmark/branch the last step repositions @ so @- is main:

```
jj edit -r xxx --ignore-immutable
<Modify the commit such as, `jj describe or `vi README.md`>
jj git push --bookmark main
jj new main
```

A complete example, the `jj log` commands are to just give
a little more visibility. The thing I'm changing is the conventaional
commit type for of vnsyoswv is "feat" is should be "docs":
```
wink@3900x 26-03-13T17:32:17.819Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
$ jj log -r ::@
@  uxuqmtov wink@saville.com 2026-03-13 10:53:15 d4205bc4
│  (empty) (no description set)
◆  plkoouwq wink@saville.com 2026-03-13 10:50:54 main e76950c0
│  docs: Update README.md with force push example
◆  vnsyoswv wink@saville.com 2026-03-13 10:32:32 525123b1
│  feat: Update README.md
◆  vuwzvmwm wink@saville.com 2026-03-13 09:38:22 1a79f803
│  feat: Initial commit for the vibe coding main repo
◆  zzzzzzzz root() 00000000
wink@3900x 26-03-13T17:57:13.692Z:~/data/prgs/rust/vc-template-x1 ((main))
$ jj edit -r vn --ignore-immutable 
Working copy  (@) now at: vnsyoswv 525123b1 feat: Update README.md
Parent commit (@-)      : vuwzvmwm 1a79f803 feat: Initial commit for the vibe coding main repo
Added 0 files, modified 1 files, removed 0 files
wink@3900x 26-03-13T17:57:27.856Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
Rebased 1 descendant commits
Working copy  (@) now at: vnsyoswv 1b6ed25c docs: Update README.md
Parent commit (@-)      : vuwzvmwm 1a79f803 feat: Initial commit for the vibe coding main repo
wink@3900x 26-03-13T17:58:34.975Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
$ jj log
○  plkoouwq wink@saville.com 2026-03-13 10:58:34 main* bc66029d
│  docs: Update README.md with force push example
@  vnsyoswv wink@saville.com 2026-03-13 10:57:53 1b6ed25c
│  docs: Update README.md
│ ◆  plkoouwq/1 wink@saville.com 2026-03-13 10:50:54 main@origin e76950c0 (hidden)
│ │  docs: Update README.md with force push example
│ ~  (elided revisions)
├─╯
◆  vuwzvmwm wink@saville.com 2026-03-13 09:38:22 1a79f803
│  feat: Initial commit for the vibe coding main repo
~
wink@3900x 26-03-13T18:15:39.052Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
$ jj log -r ::main
○  plkoouwq wink@saville.com 2026-03-13 10:58:34 main* bc66029d
│  docs: Update README.md with force push example
@  vnsyoswv wink@saville.com 2026-03-13 10:57:53 1b6ed25c
│  docs: Update README.md
◆  vuwzvmwm wink@saville.com 2026-03-13 09:38:22 1a79f803
│  feat: Initial commit for the vibe coding main repo
◆  zzzzzzzz root() 00000000
wink@3900x 26-03-13T18:17:20.926Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1a79f803025f75fb557a7b6f9d29e3dbee6a1724))
$ jj git push --bookmark main
Changes to push to origin:
  Move sideways bookmark main from e76950c0c352 to bc66029d050c
git: Enumerating objects: 8, done.
git: Counting objects: 100% (8/8), done.
git: Delta compression using up to 24 threads
git: Compressing objects: 100% (6/6), done.
git: Writing objects: 100% (6/6), 3.50 KiB | 3.50 MiB/s, done.
git: Total 6 (delta 3), reused 0 (delta 0), pack-reused 0 (from 0)
remote: Resolving deltas: 100% (3/3), completed with 1 local object.
Warning: The working-copy commit in workspace 'default' became immutable, so a new commit has been created on top of it.
Working copy  (@) now at: srxnytso 22165d77 (empty) (no description set)
Parent commit (@-)      : vnsyoswv 1b6ed25c docs: Update README.md
wink@3900x 26-03-13T18:19:07.922Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1b6ed25cf716ba3686bed15085f0463590a6200c))
$ 
wink@3900x 26-03-13T18:22:21.776Z:~/data/prgs/rust/vc-template-x1 ((jj/keep/1b6ed25cf716ba3686bed15085f0463590a6200c))
$ jj new main
Working copy  (@) now at: vytkmroy 8df04518 (empty) (no description set)
Parent commit (@-)      : plkoouwq bc66029d main | docs: Update README.md with force push example
Added 0 files, modified 1 files, removed 0 files
wink@3900x 26-03-13T18:25:23.243Z:~/data/prgs/rust/vc-template-x1 ((main))
$ jj log -r ::@
@  vytkmroy wink@saville.com 2026-03-13 11:25:23 8df04518
│  (empty) (no description set)
◆  plkoouwq wink@saville.com 2026-03-13 10:58:34 main bc66029d
│  docs: Update README.md with force push example
◆  vnsyoswv wink@saville.com 2026-03-13 10:57:53 1b6ed25c
│  docs: Update README.md
◆  vuwzvmwm wink@saville.com 2026-03-13 09:38:22 1a79f803
│  feat: Initial commit for the vibe coding main repo
◆  zzzzzzzz root() 00000000
wink@3900x 26-03-13T18:25:46.005Z:~/data/prgs/rust/vc-template-x1 ((main))
$
```

## Why `jj log` shows fewer commits than `gitk`

If you're coming from git, jj's log output can be surprising compared to
tools like `gitk --all`.

jj tracks *changes* (identified by change IDs), not individual git commits.
When you rewrite a change (`jj describe`, `jj rebase`, `jj squash`, etc.),
jj creates a new git commit and keeps the old one under `refs/jj/keep/*` as
undo history. `gitk --all` sees all of these obsolete commits; `jj log` only
shows the current version of each change.

## Useful commands

| Command | Description |
|---------|-------------|
| `jj log` | Show recent visible commits (default revset) |
| `jj log -r ::@` | Show **all** ancestors of the working copy |
| `jj log -r 'all()'` | Show all non-hidden commits (needed if you have multiple heads/branches) |
| `jj st` | Show the status of the Working and Parent commits |
| `jj st -r <chid>` | Status of the commit, `<chid>` such as `@`, `@-`, `xyz` |
| `jj show` | Show the Working commit (`-r @`) |
| `jj show -r <chid>` | Show the commit, `<chid>` such as `@`, `@-`, `xyz` |
| `jj evolog -r <chid>` | Show the evolution history of a single change |
| `jj op log` | Show operation history (each rewrite operation) |


In a single-branch workflow, `jj log -r ::@` and `jj log -r 'all()'` give
the same result. Use `all()` when you have multiple branches or heads.

## Revsets

Revsets (sets of revisions) are how jj addresses commits. A
revision is identified by `@`, a chid, or a cid, and revsets are
built from those identifiers plus the operators `-`, `+`, `..`,
and `::`. There is a complete language available — see
`jj help -k revsets`. The [`substep-test.sh`](substep-test.sh)
script scaffolds the example repo used below.

Summary:

- A revision (rev) is used for addressing/referring to commits.
- jj supports a change_id (chid) and commit_id (cid).
  - The chid is permanent and doesn't change.
  - The cid is the SHA of the current commit and changes with any
    change to it or its ancestors.
- A revset is a set of revisions.
- In many jj commands revsets can address multiple commits.
- There is one root commit; chid=zzz sha=0 owner=root.
- The root commit never has content nor description.
- There can be multiple independent lineages of commits off root():
  - to create:
    - `jj new 'root()'`
    - `git switch --orphan <name>`
  - to see these:
    - `jj log -r 'roots(~root())'`
    - `git log --all --max-parents=0 --oneline`

The examples below show these primitives in action.

### A jj repo with some commits

Some examples of revisions/rev:
   - @, chid=tkpvsrop, cid=ad2ba9b4 is the current commit
   - @-, chid=wxtmosqz, cid=35ac2422 is the ancestor of the current commit
   - @--, chid=vktlnyvm, cid=a0b0285e is the ancestor of the ancestor and so on

```
$ jj log -r 'all()'
@  tkpvsrop wink@saville.com 2026-05-01 08:41:05 ad2ba9b4
│  count 3
○  wxtmosqz wink@saville.com 2026-05-01 08:41:05 35ac2422
│  count 2
○  vktlnyvm wink@saville.com 2026-05-01 08:41:05 a0b0285e
│  count 1
○  nvwytkrw wink@saville.com 2026-05-01 08:41:05 base 54fd18f7
│  count 0
◆  zzzzzzzz root() 00000000
```

Move @ to vktlnyvm. (This could also have been done with `jj edit -r @--`.)

```
$ jj edit -r vkt
Working copy  (@) now at: vktlnyvm a0b0285e count 1
Parent commit (@-)      : nvwytkrw 54fd18f7 base | count 0
```

### Referencing commits with relative revsets

```
$ jj log -r @
@  vktlnyvm count 1
$ jj log -r @-
○  nvwytkrw base | count 0
$ jj log -r @--
◆  zzzzzzzz root() 00000000
$ jj log -r @+
○  wxtmosqz count 2
$ jj log -r @++
○  tkpvsrop count 3
$ jj log -r @+++      # empty — nothing that far out
$ jj log -r @---      # empty
$ jj log -r @..
○  tkpvsrop count 3
○  wxtmosqz count 2
$ jj log -r @::
○  tkpvsrop count 3
○  wxtmosqz count 2
@  vktlnyvm count 1
$ jj log -r ..@
@  vktlnyvm count 1
○  nvwytkrw base | count 0
$ jj log -r ::@
@  vktlnyvm count 1
○  nvwytkrw base | count 0
◆  zzzzzzzz root() 00000000
```

#### Interpretation

- `@-` and `@--` resolve to single revisions: parent and
  grandparent of @. `@+` and `@++` are children. The blank output
  for `@+++` and `@---` is the empty revset — there is no revision
  that far away in this chain.
- `@..` and `@::` are ranges going outward from @ toward
  descendants:
  - `@..` is the *open* form: descendants of @, **excluding** @
    itself (here: count 2 and count 3).
  - `@::` is the *closed* form: descendants of @, **including** @
    itself (here: @=count 1, count 2, count 3).
- `..@` and `::@` are ranges going inward from some implicit start
  toward ancestors of @:
  - `..@` includes @ and its ancestors but **excludes** the root
    commit (here: @=count 1, base=count 0).
  - `::@` includes the root commit as well.

Mnemonic: `..` and `::` both produce ranges; `::` includes the
implicit endpoint (root or visible heads), `..` excludes it. The
named operand is always part of the result on the target side; on
the source side it depends on which dot-form is used (excluded by
`..`, included by `::`).

### Referencing commits with absolute revsets

```
$ jj log -r v          # unambiguous prefix of vktlnyvm
@  vktlnyvm count 1
$ jj log -r vktln
@  vktlnyvm count 1
$ jj log -r w+         # w matches wxtmosqz, + takes its child
○  tkpvsrop count 3
$ jj log -r w--
○  nvwytkrw base | count 0
$ jj log -r w..
○  tkpvsrop count 3
$ jj log -r ..w
○  wxtmosqz count 2
@  vktlnyvm count 1
○  nvwytkrw base | count 0
$ jj log -r w::
○  tkpvsrop count 3
○  wxtmosqz count 2
$ jj log -r ::w
○  wxtmosqz count 2
@  vktlnyvm count 1
○  nvwytkrw base | count 0
◆  zzzzzzzz root() 00000000
```

#### Interpretation

- `v` and `vktln` both resolve to chid `vktlnyvm` because they are
  unambiguous prefixes within this repo — no other chid starts
  with those letters.
- `w+` resolves in two stages: `w` matches `wxtmosqz` (count 2) by
  prefix, then `+` takes its child, giving count 3.
- `w--` grandparent of w.
- `w..` descendants of w, excluding w.
- `..w` ancestors of w, not including root.
- `::w` ancestors of w, including root.
- `w::` descendants of w, including w.

Prefix rule: jj rejects ambiguous prefixes. If two chids both
start with the same letters, the prefix must be lengthened until
exactly one chid matches.

## Cross-repo Linking with Git Trailers

Each commit cross-references its counterpart in the other repo
via an `ochid:` git trailer. That is a cross-repo convention
rather than a jj mechanic — for the full definition (trailer
syntax, per-commit mechanics, `.vc-config.toml`) see
[Cross-repo linking (ochid trailers)](../AGENTS.md#cross-repo-linking-ochid-trailers)
in AGENTS.md.
