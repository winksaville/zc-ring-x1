#!/usr/bin/env python3
"""reflow.py rewraps Rust comment blocks to a column width, moving lines and never words.

Usage:

    python3 notes/reflow.py [--width N] FILE [LINE ...]

- With LINE numbers, reflow.py rewraps the comment blocks holding those lines. Without them, it
  rewraps every comment block in FILE.
- A comment block is a run of lines that each start, after indentation, with the same `///`,
  `//!`, or `//`. A comment that follows code on its line, such as an `// OK:` comment, is not a
  block and is left as it is.
- reflow.py keeps a block's structure: paragraphs, `-` and `1.` list items at any depth with their
  continuation lines, `#` headings, blank comment lines, reference definitions, and fenced code,
  which it leaves untouched.
- A word longer than the width stays whole on its own line, since Line widths lets a line that
  reads better long stay long.
- reflow.py checks that each block's words are the same, in the same order, after the rewrap, and
  stops without writing the file when they are not.

The width is prose.md's Line widths for source, 100 by default.
"""

import re
import sys
import textwrap

COMMENT = re.compile(r'^(\s*)(///|//!|//)( |$)')
ITEM = re.compile(r'^( *)(- |\d+\. )')


def prefix_of(line):
    """prefix_of returns a comment line's indentation and marker, or None for any other line."""
    m = COMMENT.match(line)
    return m.group(1) + m.group(2) if m else None


def blocks_of(lines):
    """blocks_of returns the (start, end) line index ranges, inclusive, of every comment block."""
    out, i = [], 0
    while i < len(lines):
        pre = prefix_of(lines[i])
        if pre is None:
            i += 1
            continue
        j = i
        while j + 1 < len(lines) and prefix_of(lines[j + 1]) == pre:
            j += 1
        out.append((i, j))
        i = j + 1
    return out


def reflow_block(block, width):
    """reflow_block returns a comment block's lines rewrapped to `width` columns."""
    pre = prefix_of(block[0])
    texts = [line[len(pre) + 1:] if line[len(pre):].startswith(' ') else '' for line in block]
    # Each unit is ('raw', text) kept as it is, or ('wrap', first indent, next indent, words).
    units, fenced = [], False
    for t in texts:
        if t.lstrip().startswith('```'):
            fenced = not fenced
            units.append(('raw', t))
        elif fenced or t == '' or t.startswith('#') or re.match(r'^\[[^\]]+\]: ', t):
            units.append(('raw', t))
        elif m := ITEM.match(t):
            lead = m.group(1) + m.group(2)
            units.append(['wrap', lead, ' ' * len(lead), t[len(lead):]])
        elif units and units[-1][0] == 'wrap' and t.startswith(units[-1][2]) and (
                t[len(units[-1][2]):][:1] != ' ') and not ITEM.match(t):
            units[-1][3] += ' ' + t.strip()
        else:
            units.append(['wrap', '', '', t])
    out = []
    for u in units:
        if u[0] == 'raw':
            out.append(pre + (' ' + u[1] if u[1] else ''))
            continue
        _, first, rest, words = u
        out += textwrap.wrap(words, width, initial_indent=pre + ' ' + first,
                             subsequent_indent=pre + ' ' + rest, break_long_words=False,
                             break_on_hyphens=False)
    return out


def words(lines):
    """words returns the words of comment lines, markers removed, for the same-words check."""
    return ' '.join(line[len(prefix_of(line)):] for line in lines).split()


def reflow(path, linenos=None, width=100):
    """reflow rewraps the comment blocks of `path` holding `linenos`, or all of them, in place."""
    lines = open(path).read().split('\n')
    ranges = blocks_of(lines)
    if linenos:
        want = {n - 1 for n in linenos}
        ranges = [(a, b) for a, b in ranges if any(a <= n <= b for n in want)]
    for a, b in reversed(ranges):
        new = reflow_block(lines[a:b + 1], width)
        if words(new) != words(lines[a:b + 1]):
            sys.exit(f'{path}:{a + 1}: the rewrap changed the words, nothing written')
        lines[a:b + 1] = new
    open(path, 'w').write('\n'.join(lines))


def main(argv):
    width = 100
    if argv[:1] == ['--width']:
        width, argv = int(argv[1]), argv[2:]
    if not argv:
        sys.exit(__doc__)
    reflow(argv[0], [int(n) for n in argv[1:]] or None, width)


if __name__ == '__main__':
    main(sys.argv[1:])
