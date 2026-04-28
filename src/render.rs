use std::fmt::Write;

use crate::{Block, BlockKind, DiffResult, DiffSpan, SpanTag, TypstLabel};

const PREAMBLE: &str = r##"#let diff-added(body) = {
  set text(fill: rgb("#0000ff"))
  underline(body)
}
#let diff-deleted(body) = {
  set text(fill: rgb("#cc0000"))
  strike(body)
}
"##;

/// Render a list of diff results into a compilable Typst source string.
pub fn render(results: &[DiffResult]) -> String {
    let mut out = String::new();
    out.push_str(PREAMBLE);
    out.push('\n');

    for result in results {
        match result {
            DiffResult::Unchanged(block) => {
                render_block(block, &mut out);
                out.push('\n');
            }
            DiffResult::Added(block) => {
                render_added_or_deleted(block, DiffTag::Added, &mut out);
                out.push('\n');
            }
            DiffResult::Deleted(block) => {
                render_added_or_deleted(block, DiffTag::Deleted, &mut out);
                out.push('\n');
            }
            DiffResult::Modified { kind, spans } => {
                render_modified(kind, spans, &mut out);
                out.push('\n');
            }
        }
        // Always add a blank line between blocks (paragraph break).
        // Parbreaks are filtered out before diffing, so we insert them
        // unconditionally here instead.
        out.push('\n');
    }

    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffTag {
    Added,
    Deleted,
}

impl DiffTag {
    fn func_name(self) -> &'static str {
        match self {
            DiffTag::Added => "diff-added",
            DiffTag::Deleted => "diff-deleted",
        }
    }
}

/// Render a block unchanged.
fn render_block(block: &Block, out: &mut String) {
    match block {
        Block::Paragraph { source_text } => out.push_str(source_text),
        Block::Heading { depth, body_text } => {
            write_heading_prefix(*depth, out);
            out.push_str(body_text);
        }
        Block::ListItem { body_text } => {
            out.push_str("- ");
            out.push_str(body_text);
        }
        Block::EnumItem { number, body_text } => {
            write_enum_prefix(*number, out);
            out.push_str(body_text);
        }
        Block::TermItem { term, description } => {
            out.push_str("/ ");
            out.push_str(term);
            out.push_str(": ");
            out.push_str(description);
        }
        Block::RawBlock { content } => out.push_str(content),
        Block::Equation { content, .. } => out.push_str(content),
        Block::FuncCall { content } => out.push_str(content),
        Block::Parbreak => {} // newline added by caller
    }
}

/// Returns true if the text consists only of a Typst label `<...>`.
fn is_label_only(text: &str) -> bool {
    TypstLabel::is_only(text)
}

/// Render a block wrapped in #diff-added[...] or #diff-deleted[...].
fn render_added_or_deleted(block: &Block, tag: DiffTag, out: &mut String) {
    let escape_refs = tag == DiffTag::Deleted;
    let func = tag.func_name();

    match block {
        Block::Paragraph { source_text } => {
            // Labels must be output bare so they attach to the preceding element.
            // Deleted labels are suppressed to avoid duplicates with added labels.
            if is_label_only(source_text) {
                if tag == DiffTag::Added {
                    out.push_str(source_text.trim());
                }
                return;
            }
            write!(
                out,
                "#{}[{}]",
                func,
                escape_content(source_text, escape_refs)
            )
            .unwrap();
        }
        Block::Heading { depth, body_text } => {
            write_heading_prefix(*depth, out);
            write!(out, "#{}[{}]", func, escape_content(body_text, escape_refs)).unwrap();
        }
        Block::ListItem { body_text } => {
            out.push_str("- ");
            write!(out, "#{}[{}]", func, escape_content(body_text, escape_refs)).unwrap();
        }
        Block::EnumItem { number, body_text } => {
            write_enum_prefix(*number, out);
            write!(out, "#{}[{}]", func, escape_content(body_text, escape_refs)).unwrap();
        }
        Block::TermItem { term, description } => {
            out.push_str("/ ");
            write!(
                out,
                "#{}[{}: {}]",
                func,
                escape_content(term, escape_refs),
                escape_content(description, escape_refs)
            )
            .unwrap();
        }
        // Raw blocks and equations are content-mode; they can be wrapped.
        Block::RawBlock { content } | Block::Equation { content, .. } => {
            write!(out, "#{}[{}]", func, escape_content(content, escape_refs)).unwrap();
        }
        // FuncCall represents code-mode expressions (#import, #show, etc.).
        // Wrapping them in a content block would change their semantics,
        // so we output added code as-is and deleted code as comments.
        Block::FuncCall { content } => {
            if tag == DiffTag::Added {
                out.push_str(content);
            } else {
                for line in content.lines() {
                    write!(out, "// {line}").unwrap();
                    out.push('\n');
                }
                // Remove the trailing newline since the caller adds one.
                if out.ends_with('\n') {
                    out.pop();
                }
            }
        }
        Block::Parbreak => {} // paragraph breaks have no visual content to mark
    }
}

/// Render a modified block with per-span diff markup.
fn render_modified(kind: &BlockKind, spans: &[DiffSpan], out: &mut String) {
    match kind {
        BlockKind::Heading { depth } => write_heading_prefix(*depth, out),
        BlockKind::ListItem => out.push_str("- "),
        BlockKind::EnumItem { number } => write_enum_prefix(*number, out),
        BlockKind::TermItem => out.push_str("/ "),
        BlockKind::Paragraph => {}
        BlockKind::Atomic | BlockKind::Parbreak => {}
    }
    render_spans(spans, out);
}

/// Render diff spans with markup for deleted/inserted text.
fn render_spans(spans: &[DiffSpan], out: &mut String) {
    let mut prev_was_diff = false;
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        match span.tag {
            SpanTag::Equal => {
                // After `#diff-added[...]` or `#diff-deleted[...]`, a `(` or `[`
                // would be parsed as function arguments by Typst. Insert a
                // zero-width space to break the call syntax.
                if prev_was_diff && (span.text.starts_with('(') || span.text.starts_with('[')) {
                    out.push('\u{200B}');
                }
                out.push_str(&span.text);
                prev_was_diff = false;
            }
            SpanTag::Deleted => {
                if is_label_only(&span.text) {
                    prev_was_diff = false;
                    continue;
                }
                write!(out, "#diff-deleted[{}]", escape_content(&span.text, true)).unwrap();
                prev_was_diff = true;
            }
            SpanTag::Inserted => {
                if is_label_only(&span.text) {
                    out.push_str(&span.text);
                    prev_was_diff = false;
                    continue;
                }
                write!(out, "#diff-added[{}]", escape_content(&span.text, false)).unwrap();
                prev_was_diff = true;
            }
        }
    }
}

/// Write the heading prefix (`= `, `== `, etc.).
fn write_heading_prefix(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push('=');
    }
    out.push(' ');
}

/// Write the enum item prefix (`+ ` for auto, `{n}. ` for explicit).
fn write_enum_prefix(number: Option<usize>, out: &mut String) {
    match number {
        Some(n) => write!(out, "{}. ", n).unwrap(),
        None => out.push_str("+ "),
    }
}

/// Escape content so it can safely be placed inside a Typst content block `[...]`.
///
/// - Tracks bracket depth so that balanced `[...]` pairs are left untouched.
/// - Unbalanced `]` is escaped as `\]`.
/// - If the content ends with an odd number of backslashes, a trailing space is
///   appended to prevent the closing `]` from being interpreted as `\]`.
/// - When `escape_refs` is true, `@` is escaped as `\@` and `<` is escaped
///   as `\<` to suppress reference resolution and label creation (used for
///   deleted content where the referenced label may no longer exist or would
///   create duplicates).
fn escape_content(s: &str, escape_refs: bool) -> String {
    let mut result = String::with_capacity(s.len());
    let mut depth: i32 = 0;
    for ch in s.chars() {
        match ch {
            '[' => {
                depth += 1;
                result.push(ch);
            }
            ']' => {
                if depth > 0 {
                    depth -= 1;
                    result.push(ch);
                } else {
                    result.push('\\');
                    result.push(']');
                }
            }
            '@' if escape_refs => {
                result.push('\\');
                result.push('@');
            }
            '<' if escape_refs => {
                result.push('\\');
                result.push('<');
            }
            _ => result.push(ch),
        }
    }
    // If the result ends with an odd number of backslashes, the closing `]`
    // added by the caller would be interpreted as `\]` (an escaped bracket).
    // Append a space to break the escape sequence.
    let trailing_backslashes = result.chars().rev().take_while(|&c| c == '\\').count();
    if trailing_backslashes % 2 != 0 {
        result.push(' ');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_content_no_special() {
        assert_eq!(escape_content("hello world", false), "hello world");
    }

    #[test]
    fn test_escape_content_balanced_brackets() {
        assert_eq!(escape_content("a [b] c", false), "a [b] c");
    }

    #[test]
    fn test_escape_content_unbalanced_bracket() {
        assert_eq!(escape_content("a ] b", false), "a \\] b");
    }

    #[test]
    fn test_escape_content_trailing_backslash() {
        assert_eq!(escape_content("text\\", false), "text\\ ");
    }

    #[test]
    fn test_escape_content_trailing_double_backslash() {
        assert_eq!(escape_content("text\\\\", false), "text\\\\");
    }

    #[test]
    fn test_escape_content_refs_not_escaped_by_default() {
        assert_eq!(escape_content("see @ref here", false), "see @ref here");
    }

    #[test]
    fn test_escape_content_refs_escaped_when_requested() {
        assert_eq!(escape_content("see @ref here", true), "see \\@ref here");
    }

    #[test]
    fn test_escape_content_labels_escaped_when_requested() {
        assert_eq!(escape_content("text <my-label>", true), "text \\<my-label>");
    }

    #[test]
    fn test_is_label_only() {
        assert!(is_label_only("<my-label>"));
        assert!(is_label_only("  <my-label>  "));
        assert!(!is_label_only("text <label>"));
        assert!(!is_label_only("<a> <b>"));
        assert!(!is_label_only("no label"));
    }

    #[test]
    fn test_render_preamble() {
        let output = render(&[]);
        assert!(output.contains("#let diff-added"));
        assert!(output.contains("#let diff-deleted"));
    }

    #[test]
    fn test_render_unchanged() {
        let results = vec![DiffResult::Unchanged(Block::Paragraph {
            source_text: "Hello".into(),
        })];
        let output = render(&results);
        assert!(output.contains("\nHello\n"));
        let body = output.split("\n\n").last().unwrap();
        assert!(!body.contains("#diff-added["));
        assert!(!body.contains("#diff-deleted["));
    }

    #[test]
    fn test_render_added_paragraph() {
        let results = vec![DiffResult::Added(Block::Paragraph {
            source_text: "New text".into(),
        })];
        let output = render(&results);
        assert!(output.contains("#diff-added[New text]"));
    }

    #[test]
    fn test_render_deleted_paragraph_escapes_refs() {
        let results = vec![DiffResult::Deleted(Block::Paragraph {
            source_text: "see @old-ref here".into(),
        })];
        let output = render(&results);
        assert!(output.contains("#diff-deleted[see \\@old-ref here]"));
    }

    #[test]
    fn test_render_label_only_paragraph_not_wrapped() {
        let results = vec![DiffResult::Added(Block::Paragraph {
            source_text: "<my-label>".into(),
        })];
        let output = render(&results);
        assert!(output.contains("<my-label>"));
        assert!(!output.contains("#diff-added[<my-label>]"));
    }

    #[test]
    fn test_render_modified() {
        let results = vec![DiffResult::Modified {
            kind: BlockKind::Paragraph,
            spans: vec![
                DiffSpan {
                    tag: SpanTag::Equal,
                    text: "Hello ".into(),
                },
                DiffSpan {
                    tag: SpanTag::Deleted,
                    text: "world".into(),
                },
                DiffSpan {
                    tag: SpanTag::Inserted,
                    text: "there".into(),
                },
            ],
        }];
        let output = render(&results);
        assert!(output.contains("Hello #diff-deleted[world]#diff-added[there]"));
    }

    #[test]
    fn test_render_modified_label_bare() {
        let results = vec![DiffResult::Modified {
            kind: BlockKind::Paragraph,
            spans: vec![
                DiffSpan {
                    tag: SpanTag::Deleted,
                    text: "<old-label>".into(),
                },
                DiffSpan {
                    tag: SpanTag::Inserted,
                    text: "<new-label>".into(),
                },
            ],
        }];
        let output = render(&results);
        assert!(output.contains("<new-label>"));
        assert!(!output.contains("#diff-added[<new-label>]"));
        assert!(!output.contains("#diff-deleted[\\<old-label>]"));
    }
}
