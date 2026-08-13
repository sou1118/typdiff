# typdiff

A diff tool for [Typst](https://typst.app/) documents, similar to [latexdiff](https://github.com/ftilmann/latexdiff) for LaTeX.

`typdiff` compares two Typst source files and generates a new Typst document that visually highlights the differences — <ins>added text</ins> in blue with underline and <del>deleted text</del> in red with strikethrough.

## Example

Given an old document:

```typst
= Introduction

This is the old text.

- First item
- Second item

== Details

The quick brown fox jumps over the lazy dog.
```

And a new document:

```typst
= Background

This is the new text.

- First item
- Third item
- Fourth item

== Details

The quick red fox leaps over the lazy cat.
```

Running `typdiff old.typ new.typ` produces:

```typst
= #diff-deleted[Introduction]

= #diff-added[Background]

This is the #diff-deleted[old]#diff-added[new] text.

- First item

- #diff-deleted[Second]#diff-added[Third] item

- #diff-added[Fourth item]

== Details

The quick #diff-deleted[brown]#diff-added[red] fox #diff-deleted[jumps]#diff-added[leaps] over the lazy #diff-deleted[dog]#diff-added[cat].
```

Compile the output with `typst compile` to get a PDF with visual diff markup.

## Installation

### From crates.io

```sh
cargo install typdiff
```

### From source (latest development version)

```sh
cargo install --git https://github.com/sou1118/typdiff
```

## Usage

```
typdiff <OLD> <NEW> [-o <OUTPUT>]
```

| Argument | Description |
|---|---|
| `<OLD>` | Path to the old Typst file |
| `<NEW>` | Path to the new Typst file |
| `-o, --output <OUTPUT>` | Output file path (defaults to stdout) |

### Examples

```sh
# Print diff to stdout
typdiff old.typ new.typ

# Save diff to a file
typdiff old.typ new.typ -o diff.typ

# Generate a PDF
typdiff old.typ new.typ -o diff.typ && typst compile diff.typ
```

## Python

`typdiff` is also available as a Python package.

### Installation

```sh
pip install typdiff
```

### Usage

```python
import typdiff

diff = typdiff.diff(old_bytes, new_bytes)  # from bytes
diff = typdiff.diff_files("old.typ", "new.typ")  # from file paths (str or Path)
```

`diff()` takes and returns `bytes`, matching how [`typst`](https://pypi.org/project/typst/)'s `compile()` treats `bytes` as inline source (a plain `str` argument is instead treated as a path to read) — so `typdiff`'s output can be passed straight into `compile()`. `diff_files()` still takes file paths as `str`/`Path`, since those are paths rather than document content.

### Producing a PDF

Combine `typdiff` with `typst` (Python bindings for the Typst compiler, `pip install typst`) to go straight from two Typst files to a diff PDF, without shelling out to either CLI:

```python
import typdiff
import typst

pdf_bytes = typst.compile(typdiff.diff_files("old.typ", "new.typ"))
```

## Features

- **Block-level diffing** — Detects structural changes in headings, paragraphs, list items, enum items, and term list items
- **Word-level diffing** — Shows fine-grained changes within modified blocks
- **CJK support** — Character-level granularity for Chinese, Japanese, and Korean text
- **Atomic blocks** — Raw blocks, equations, and function calls are treated as indivisible units
- **Safe output** — Escapes references, labels, and special syntax in deleted content to prevent compilation errors

## How it works

1. **Parse** — Both Typst files are parsed into block-level elements (headings, paragraphs, list items, etc.) using `typst-syntax`
2. **Diff** — Blocks are compared using the Patience diff algorithm, with similarity-based matching for modified regions
3. **Render** — The diff result is rendered back to Typst source with `#diff-added[...]` and `#diff-deleted[...]` markup

## License

Apache-2.0
