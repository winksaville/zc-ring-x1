#!/usr/bin/env python3
"""Report authored punctuation the prose rules ban.

The rules are agent-data/prose.md's Semicolons and Typeable punctuation
only. A byte scan cannot enforce them, since a semicolon is syntax in
code, so this blanks what is code and expects zero elsewhere:

- Markdown: a fenced block is code apart from its comments, and an
  inline code span is code. A banned character counts anywhere, a span
  included, since a span is not exempt by itself.
- Rust and TOML: a banned character counts anywhere. A semicolon counts
  in a comment, outside a code span and outside the code of a doctest
  fence, and in a string literal when it sits between two words.

Usage: prose-check.py [FILE...]. With no FILE it checks every tracked
file that is not excluded below. Exit status 1 when anything is found.
"""

import re
import subprocess
import sys

# The characters are spelled as escapes so this file passes its own check.
BANNED = "—–…→"
SPAN = re.compile(r"`[^`]*`")
STRING_SEMI = re.compile(r"[a-z]; [a-z]")

# Frozen history, the agent-files (their hits are specimens naming the
# characters), and files that are not our prose.
EXCLUDED = (
    "notes/chores/",
    "notes/done.md",
    "tprobe/notes/chores/",
    "agent-data/",
    "AGENTS.md",
    "CLAUDE.md",
    "custom.md",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "Cargo.lock",
    ".gitignore",
)

# Transcribed text keeps its characters: (file, text the line contains).
TRANSCRIBED = (("README.md", "Rust latency microbenchmark harness"),)

CHECKED = (".md", ".rs", ".toml", ".sh", ".py")


def has_banned(line):
    """True when the line holds a banned character."""
    return any(c in line for c in BANNED)


def check_markdown(lines):
    """Yield (number, line) for each offending line of a markdown file."""
    fence = False
    for n, line in enumerate(lines, 1):
        if line.lstrip().startswith("```"):
            fence = not fence
            continue
        if fence:
            m = re.search(r"(//|#\s).*", line)
            prose = m.group(0) if m else ""
        else:
            prose = line
        if has_banned(line) or ";" in SPAN.sub("", prose):
            yield n, line


def check_source(lines, marker):
    """Yield (number, line) for each offending line of a source file.

    `marker` is the comment marker, `//` for Rust and `#` otherwise.
    """
    fence = False
    lead = re.compile(r"\s*(" + re.escape(marker) + r"[/!]?)(.*)")
    trail = re.compile(r"\s(" + re.escape(marker) + r")(?![/!])(.*)$")
    for n, line in enumerate(lines, 1):
        hit = has_banned(line)
        m = lead.match(line) or trail.search(line)
        if m:
            body = m.group(2)
            if body.strip().startswith("```"):
                fence = not fence
                body = ""
            elif fence:
                inner = re.search(re.escape(marker) + r"(.*)", body)
                body = inner.group(1) if inner else ""
            if ";" in SPAN.sub("", body):
                hit = True
        elif marker == "//" and STRING_SEMI.search(line):
            hit = True
        if hit:
            yield n, line


def check(path):
    """Return the offending (number, line) pairs of one file."""
    with open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    if path.endswith(".md"):
        found = check_markdown(lines)
    else:
        found = check_source(lines, "//" if path.endswith(".rs") else "#")
    kept = [text for name, text in TRANSCRIBED if name == path]
    return [(n, l) for n, l in found if not any(t in l for t in kept)]


def tracked():
    """Return the tracked files this check covers."""
    out = subprocess.run(
        ["git", "ls-files"], check=True, capture_output=True, text=True
    ).stdout
    return [
        p
        for p in out.split("\n")
        if p.endswith(CHECKED) and not p.startswith(EXCLUDED)
    ]


def main():
    """Check the named files, or every covered file, and report."""
    paths = sys.argv[1:] or tracked()
    count = 0
    for path in paths:
        for n, line in check(path):
            print(f"{path}:{n}: {line.rstrip()}")
            count += 1
    if count:
        print(f"prose-check: {count} line(s) owe punctuation", file=sys.stderr)
        return 1
    print(f"prose-check: {len(paths)} file(s) clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
