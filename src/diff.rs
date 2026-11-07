use similar::{Algorithm, DiffOp, TextDiff};
use typst_syntax::{SyntaxKind, SyntaxNode};

use crate::{Block, DiffResult, DiffSpan, SpanTag, TypstLabel};

/// Diff two sequences of blocks, returning a list of diff results.
///
/// Uses Patience diff at the block level and word-level diff for modified blocks.
pub fn diff(old_blocks: &[Block], new_blocks: &[Block]) -> Vec<DiffResult> {
    let old_sigs: Vec<String> = old_blocks.iter().map(|b| b.signature()).collect();
    let new_sigs: Vec<String> = new_blocks.iter().map(|b| b.signature()).collect();

    let old_refs: Vec<&str> = old_sigs.iter().map(|s| s.as_str()).collect();
    let new_refs: Vec<&str> = new_sigs.iter().map(|s| s.as_str()).collect();

    let text_diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_slices(&old_refs, &new_refs);

    let mut results = Vec::new();

    for op in text_diff.ops() {
        match *op {
            DiffOp::Equal { old_index, len, .. } => {
                for b in &old_blocks[old_index..(old_index + len)] {
                    results.push(DiffResult::Unchanged(b.clone()));
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for b in &old_blocks[old_index..(old_index + old_len)] {
                    results.push(DiffResult::Deleted(b.clone()));
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for b in &new_blocks[new_index..(new_index + new_len)] {
                    results.push(DiffResult::Added(b.clone()));
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                process_replace(
                    &old_blocks[old_index..(old_index + old_len)],
                    &new_blocks[new_index..(new_index + new_len)],
                    &mut results,
                );
            }
        }
    }

    results
}

/// Compute character-level similarity ratio between two strings (0.0–1.0).
fn text_similarity(a: &str, b: &str) -> f32 {
    TextDiff::from_chars(a, b).ratio()
}

/// Process a Replace operation using similarity-based greedy matching.
///
/// Instead of pairing blocks by position, we compute similarity scores for all
/// compatible pairs and greedily match the most similar ones first. This produces
/// better word-level diffs when blocks are inserted or removed within a Replace region.
fn process_replace(old_range: &[Block], new_range: &[Block], results: &mut Vec<DiffResult>) {
    // 1. Build candidate pairs with similarity scores.
    let mut candidates: Vec<(usize, usize, f32)> = Vec::new();
    for (oi, old_b) in old_range.iter().enumerate() {
        if old_b.is_atomic() {
            continue;
        }
        let old_text = match old_b.diff_text() {
            Some(t) => t,
            None => continue,
        };
        for (ni, new_b) in new_range.iter().enumerate() {
            if new_b.is_atomic() || old_b.kind() != new_b.kind() {
                continue;
            }
            let new_text = match new_b.diff_text() {
                Some(t) => t,
                None => continue,
            };
            let ratio = text_similarity(&old_text, &new_text);
            if ratio >= 0.5 {
                candidates.push((oi, ni, ratio));
            }
        }
    }

    // 2. Greedy match: sort by similarity descending, assign pairs.
    candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    // matched_old[oi] = Some(ni), matched_new[ni] = Some(oi)
    let mut matched_old: Vec<Option<usize>> = vec![None; old_range.len()];
    let mut matched_new: Vec<Option<usize>> = vec![None; new_range.len()];

    for (oi, ni, _ratio) in &candidates {
        if matched_old[*oi].is_none() && matched_new[*ni].is_none() {
            matched_old[*oi] = Some(*ni);
            matched_new[*ni] = Some(*oi);
        }
    }

    // 3. Emit results in document order.
    // First: unmatched old blocks as Deleted.
    for (oi, old_b) in old_range.iter().enumerate() {
        if matched_old[oi].is_none() {
            results.push(DiffResult::Deleted(old_b.clone()));
        }
    }

    // Then: new blocks in order — Modified if matched, Added if unmatched.
    for (ni, new_b) in new_range.iter().enumerate() {
        if let Some(oi) = matched_new[ni] {
            let old_b = &old_range[oi];
            let old_text = old_b.diff_text().unwrap();
            let new_text = new_b.diff_text().unwrap();
            let spans = word_diff(&old_text, &new_text);
            results.push(DiffResult::Modified {
                kind: old_b.kind(),
                spans,
            });
        } else {
            results.push(DiffResult::Added(new_b.clone()));
        }
    }
}

/// Tokenize text for diffing: runs of non-CJK alphanumeric characters form word
/// tokens (an intraword `_` or `*`, as in `snake_case`, stays inside the word),
/// everything else (CJK characters, punctuation, whitespace) becomes an
/// individual single-character token.
///
/// Typst code expressions (`#func[...]`, `#func(...)`), references (`@label`),
/// labels (`<label>`), inline equations (`$...$`), emphasis and strong spans
/// (`_..._`, `*...*`), and escape sequences (`\x`) are treated as atomic
/// tokens so the diff never fragments valid Typst syntax.
///
/// This gives word-level granularity for Latin text while allowing
/// character-level precision for CJK text (which has no whitespace boundaries).
fn tokenize_mixed(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let emphasis = emphasis_spans(s);
    let mut next_emphasis = 0;

    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        let c_len = c.len_utf8();

        // Skip spans that begin inside a token consumed earlier (`#f[_x_]`).
        while next_emphasis < emphasis.len() && emphasis[next_emphasis].0 < i {
            next_emphasis += 1;
        }

        if next_emphasis < emphasis.len() && emphasis[next_emphasis].0 == i {
            // Emphasis or strong span: keep `_..._` / `*...*` as one token.
            let end = emphasis[next_emphasis].1;
            tokens.push(&s[i..end]);
            i = end;
        } else if c == '#' {
            // Typst code expression: #identifier followed by [...] and/or (...)
            let start = i;
            i += 1; // skip '#'
            if i < s.len() && s[i..].chars().next().unwrap().is_ascii_alphabetic() {
                // Consume identifier (letters, digits, hyphens, underscores)
                while i < s.len() {
                    let next = s[i..].chars().next().unwrap();
                    if next.is_ascii_alphanumeric() || next == '-' || next == '_' {
                        i += next.len_utf8();
                    } else {
                        break;
                    }
                }
                // Consume trailing bracket [...] and paren (...) groups
                while i < s.len() {
                    let next = s[i..].chars().next().unwrap();
                    if next == '[' {
                        i = skip_balanced(s, i, '[', ']');
                    } else if next == '(' {
                        i = skip_balanced(s, i, '(', ')');
                    } else {
                        break;
                    }
                }
                tokens.push(&s[start..i]);
            } else {
                // Standalone '#'
                tokens.push(&s[start..i]);
            }
        } else if c == '$' {
            // Inline equation: keep `$...$` as one atomic token. An unclosed
            // `$` falls back to a single-character token, like `<label>`.
            let start = i;
            i += find_closing_dollar(&s[start..]).unwrap_or(c_len);
            tokens.push(&s[start..i]);
        } else if c == '\\' {
            // Escape sequence: keep `\x` as one token so an escaped `$` in
            // markup never opens an equation.
            let start = i;
            i += c_len;
            if let Some(next) = s[i..].chars().next() {
                i += next.len_utf8();
            }
            tokens.push(&s[start..i]);
        } else if c == '@' {
            // Typst reference: @label (alphanumeric, hyphens, underscores, colons, periods)
            let start = i;
            i += 1; // skip '@'
            while i < s.len() {
                let next = s[i..].chars().next().unwrap();
                if next.is_ascii_alphanumeric() || matches!(next, '-' | '_' | ':' | '.') {
                    i += next.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(&s[start..i]);
        } else if c == '<' {
            let start = i;
            if let Some(end) = TypstLabel::end(&s[start..]) {
                i += end;
            } else {
                i += c_len;
            }
            tokens.push(&s[start..i]);
        } else if is_wordy(c) {
            // Word token. An `_` or `*` between two word characters is kept
            // inside the token: Typst treats it as literal text there, but it
            // would become a delimiter once a diff mark sits next to it.
            let start = i;
            i += c_len;
            while let Some(next) = s[i..].chars().next() {
                if is_wordy(next) {
                    i += next.len_utf8();
                } else if matches!(next, '_' | '*')
                    && s[i + 1..].chars().next().is_some_and(is_wordy)
                {
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push(&s[start..i]);
        } else {
            tokens.push(&s[i..i + c_len]);
            i += c_len;
        }
    }

    tokens
}

/// Whether `c` counts as a word character for Typst's intraword rule: alphanumeric
/// and not CJK. Mirrors `Lexer::in_word` in typst-syntax, which decides when `_`
/// and `*` are literal text instead of emphasis delimiters.
fn is_wordy(c: char) -> bool {
    use unicode_script::{Script, UnicodeScript};
    c.is_alphanumeric()
        && !matches!(
            c.script(),
            Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul
        )
}

/// Skip over a balanced delimiter group starting at position `i`.
/// Returns the position after the closing delimiter.
/// If unbalanced, returns the end of the string.
fn skip_balanced(s: &str, start: usize, open: char, close: char) -> usize {
    let mut i = start;
    let mut depth: i32 = 0;

    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        let c_len = c.len_utf8();
        if c == '\\' {
            // Skip escaped character
            i += c_len;
            if i < s.len() {
                let next = s[i..].chars().next().unwrap();
                i += next.len_utf8();
            }
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return i + c_len;
            }
        }
        i += c_len;
    }
    i
}

/// Byte length of the inline equation at the start of `s`, if it is closed.
///
/// `s` must start with `$`. Scans forward for the matching `$`, skipping
/// escaped characters (`\$`) and quoted strings (`"a $ b"`), since a `$`
/// inside either does not end the equation. Returns `None` when unclosed.
fn find_closing_dollar(s: &str) -> Option<usize> {
    let mut in_string = false;
    let mut chars = s.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '"' => in_string = !in_string,
            '$' if !in_string => return Some(i + c.len_utf8()),
            _ => {}
        }
    }
    None
}

/// Byte ranges of the outermost emphasis (`_..._`) and strong (`*...*`) spans
/// in `s`, in document order.
///
/// The ranges come from the Typst parser, so they follow Typst's own rules:
/// no emphasis inside a word (`snake_case`), none in references, labels, raw
/// text or math, and escaped delimiters are ignored. A span the parser reports
/// as erroneous (an unclosed delimiter) is left out.
fn emphasis_spans(s: &str) -> Vec<(usize, usize)> {
    // Nested diffs re-tokenize span interiors; skip the parse when no delimiter can occur.
    if !s.contains(['_', '*']) {
        return Vec::new();
    }

    fn collect(node: &SyntaxNode, mut offset: usize, out: &mut Vec<(usize, usize)>) {
        for child in node.children() {
            let end = offset + child.len();
            match child.kind() {
                SyntaxKind::Emph | SyntaxKind::Strong if !child.erroneous() => {
                    out.push((offset, end));
                }
                _ => collect(child, offset, out),
            }
            offset = end;
        }
    }

    let mut spans = Vec::new();
    collect(&typst_syntax::parse(s), 0, &mut spans);
    spans
}

/// The delimiter (`_` or `*`) of a token produced for an emphasis or strong span.
fn emphasis_delimiter(token: &str) -> Option<char> {
    let delimiter = token.chars().next().filter(|c| matches!(c, '_' | '*'))?;
    (token.len() >= 2 && token.ends_with(delimiter)).then_some(delimiter)
}

/// Perform mixed-granularity diff and return coalesced spans.
///
/// Uses a custom tokenizer that splits CJK characters individually while
/// keeping ASCII words intact, then diffs at the token level.
fn word_diff(old_text: &str, new_text: &str) -> Vec<DiffSpan> {
    coalesce_spans(diff_tokens(old_text, new_text))
}

/// Token-level diff of `old_text` against `new_text`, without coalescing.
///
/// An emphasis or strong token is atomic, so it is deleted or inserted whole.
/// When one is replaced by another with the same delimiter, the delimiters stay
/// unmarked and the text between them is diffed recursively.
fn diff_tokens(old_text: &str, new_text: &str) -> Vec<DiffSpan> {
    let old_tokens = tokenize_mixed(old_text);
    let new_tokens = tokenize_mixed(new_text);

    let diff = TextDiff::configure().diff_slices(&old_tokens, &new_tokens);

    let span = |tag, text: &str| DiffSpan {
        tag,
        text: text.to_string(),
    };
    let mut spans = Vec::new();
    for op in diff.ops() {
        match *op {
            DiffOp::Equal { old_index, len, .. } => {
                for t in &old_tokens[old_index..old_index + len] {
                    spans.push(span(SpanTag::Equal, t));
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for t in &old_tokens[old_index..old_index + old_len] {
                    spans.push(span(SpanTag::Deleted, t));
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for t in &new_tokens[new_index..new_index + new_len] {
                    spans.push(span(SpanTag::Inserted, t));
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let old = &old_tokens[old_index..old_index + old_len];
                let new = &new_tokens[new_index..new_index + new_len];
                if let ([o], [n]) = (old, new)
                    && let Some(d) = emphasis_delimiter(o)
                    && emphasis_delimiter(n) == Some(d)
                {
                    let delimiter = d.to_string();
                    spans.push(span(SpanTag::Equal, &delimiter));
                    spans.extend(diff_tokens(&o[1..o.len() - 1], &n[1..n.len() - 1]));
                    spans.push(span(SpanTag::Equal, &delimiter));
                } else {
                    for t in old {
                        spans.push(span(SpanTag::Deleted, t));
                    }
                    for t in new {
                        spans.push(span(SpanTag::Inserted, t));
                    }
                }
            }
        }
    }
    spans
}

/// Merge adjacent spans that have the same tag.
fn coalesce_spans(spans: Vec<DiffSpan>) -> Vec<DiffSpan> {
    let mut result: Vec<DiffSpan> = Vec::new();
    for span in spans {
        if let Some(last) = result.last_mut()
            && last.tag == span.tag
        {
            last.text.push_str(&span.text);
            continue;
        }
        result.push(span);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockKind;

    #[test]
    fn test_identical_blocks() {
        let blocks = vec![Block::Paragraph {
            source_text: "Hello world".into(),
        }];
        let results = diff(&blocks, &blocks);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], DiffResult::Unchanged(_)));
    }

    #[test]
    fn test_added_block() {
        let old = vec![];
        let new = vec![Block::Paragraph {
            source_text: "New text".into(),
        }];
        let results = diff(&old, &new);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], DiffResult::Added(_)));
    }

    #[test]
    fn test_deleted_block() {
        let old = vec![Block::Paragraph {
            source_text: "Old text".into(),
        }];
        let new = vec![];
        let results = diff(&old, &new);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], DiffResult::Deleted(_)));
    }

    #[test]
    fn test_modified_paragraph() {
        let old = vec![Block::Paragraph {
            source_text: "This is old text".into(),
        }];
        let new = vec![Block::Paragraph {
            source_text: "This is new text".into(),
        }];
        let results = diff(&old, &new);
        assert_eq!(results.len(), 1);
        if let DiffResult::Modified { kind, spans } = &results[0] {
            assert_eq!(*kind, BlockKind::Paragraph);
            assert!(
                spans
                    .iter()
                    .any(|s| s.tag == SpanTag::Deleted && s.text.contains("old"))
            );
            assert!(
                spans
                    .iter()
                    .any(|s| s.tag == SpanTag::Inserted && s.text.contains("new"))
            );
        } else {
            panic!("expected Modified, got {:?}", results[0]);
        }
    }

    #[test]
    fn test_atomic_blocks_not_modified() {
        let old = vec![Block::RawBlock {
            content: "```rust\nold code\n```".into(),
        }];
        let new = vec![Block::RawBlock {
            content: "```rust\nnew code\n```".into(),
        }];
        let results = diff(&old, &new);
        // Atomic blocks should stay as Delete + Insert, not Modified.
        assert!(results.iter().any(|r| matches!(r, DiffResult::Deleted(_))));
        assert!(results.iter().any(|r| matches!(r, DiffResult::Added(_))));
    }

    #[test]
    fn test_similarity_matching_with_insertion() {
        // Old: [A, B]  New: [A', X, B'] where A'/B' are minor edits, X is new.
        // Positional pairing would pair A-A', B-X (wrong). Similarity should pair A-A', B-B'.
        let old = vec![
            Block::Paragraph {
                source_text: "The first paragraph about networks.".into(),
            },
            Block::Paragraph {
                source_text: "The second paragraph about routing.".into(),
            },
        ];
        let new = vec![
            Block::Paragraph {
                source_text: "The first paragraph about network systems.".into(),
            },
            Block::Paragraph {
                source_text: "A completely new paragraph about security.".into(),
            },
            Block::Paragraph {
                source_text: "The second paragraph about routing protocols.".into(),
            },
        ];

        let mut results = Vec::new();
        process_replace(&old, &new, &mut results);

        // Should produce: Modified(A→A'), Added(X), Modified(B→B')
        // Not: Modified(A→A'), Modified(B→X), Added(B')
        let modified_count = results
            .iter()
            .filter(|r| matches!(r, DiffResult::Modified { .. }))
            .count();
        let added_count = results
            .iter()
            .filter(|r| matches!(r, DiffResult::Added(_)))
            .count();
        assert_eq!(
            modified_count, 2,
            "expected 2 Modified, got results: {results:?}"
        );
        assert_eq!(added_count, 1, "expected 1 Added, got results: {results:?}");

        // The Modified blocks should contain word-level changes, not whole-block replacements.
        if let DiffResult::Modified { spans, .. } = &results[0] {
            assert!(
                spans.iter().any(|s| s.tag == SpanTag::Equal),
                "first Modified should have Equal spans (word-level diff)"
            );
        }
    }

    #[test]
    fn test_similarity_matching_with_deletion() {
        // Old: [A, X, B]  New: [A', B'] — X was removed, A and B had minor edits.
        let old = vec![
            Block::Paragraph {
                source_text: "Introduction to the research topic.".into(),
            },
            Block::Paragraph {
                source_text: "This paragraph will be removed entirely.".into(),
            },
            Block::Paragraph {
                source_text: "Conclusion of the research topic.".into(),
            },
        ];
        let new = vec![
            Block::Paragraph {
                source_text: "Introduction to the research subject.".into(),
            },
            Block::Paragraph {
                source_text: "Conclusion of the research subject.".into(),
            },
        ];

        let mut results = Vec::new();
        process_replace(&old, &new, &mut results);

        let modified_count = results
            .iter()
            .filter(|r| matches!(r, DiffResult::Modified { .. }))
            .count();
        let deleted_count = results
            .iter()
            .filter(|r| matches!(r, DiffResult::Deleted(_)))
            .count();
        assert_eq!(
            modified_count, 2,
            "expected 2 Modified, got results: {results:?}"
        );
        assert_eq!(
            deleted_count, 1,
            "expected 1 Deleted, got results: {results:?}"
        );
    }

    #[test]
    fn test_similarity_low_ratio_no_match() {
        // Two completely different paragraphs should not be matched (ratio < 0.5).
        let old = vec![Block::Paragraph {
            source_text: "AAAA BBBB CCCC DDDD".into(),
        }];
        let new = vec![Block::Paragraph {
            source_text: "XXXX YYYY ZZZZ WWWW".into(),
        }];

        let mut results = Vec::new();
        process_replace(&old, &new, &mut results);

        // With no similarity, should fall back to Deleted + Added.
        assert!(
            results.iter().any(|r| matches!(r, DiffResult::Deleted(_))),
            "expected Deleted"
        );
        assert!(
            results.iter().any(|r| matches!(r, DiffResult::Added(_))),
            "expected Added"
        );
        assert!(
            !results
                .iter()
                .any(|r| matches!(r, DiffResult::Modified { .. })),
            "should not produce Modified for dissimilar blocks"
        );
    }

    #[test]
    fn test_similarity_different_kinds_not_paired() {
        // A Paragraph and a ListItem should not be paired even if text is similar.
        let old = vec![Block::Paragraph {
            source_text: "Some shared text here.".into(),
        }];
        let new = vec![Block::ListItem {
            body_text: "Some shared text here.".into(),
        }];

        let mut results = Vec::new();
        process_replace(&old, &new, &mut results);

        assert!(
            !results
                .iter()
                .any(|r| matches!(r, DiffResult::Modified { .. })),
            "different block kinds should not produce Modified"
        );
    }

    #[test]
    fn test_tokenize_typst_funccall_atomic() {
        let tokens = tokenize_mixed("before #footnote[https://example.com] after");
        // #footnote[...] should be a single token
        assert!(
            tokens.iter().any(|t| t.starts_with("#footnote[")),
            "expected atomic #footnote[...] token, got: {tokens:?}"
        );
    }

    #[test]
    fn test_tokenize_typst_ref_atomic() {
        let tokens = tokenize_mixed("see @my_ref_2024 for details");
        assert!(
            tokens.contains(&"@my_ref_2024"),
            "expected atomic @ref token, got: {tokens:?}"
        );
    }

    #[test]
    fn test_tokenize_typst_label_atomic() {
        let tokens = tokenize_mixed("<sample-widget_anchor>");
        assert_eq!(tokens, vec!["<sample-widget_anchor>"]);
    }

    #[test]
    fn test_tokenize_inline_equation_is_atomic() {
        let tokens = tokenize_mixed("is $t = 12.4 \"hour\"$ for");
        assert_eq!(tokens, vec!["is", " ", "$t = 12.4 \"hour\"$", " ", "for"]);
    }

    #[test]
    fn test_tokenize_escaped_dollar_in_markup_is_not_an_equation() {
        let tokens = tokenize_mixed("\\$5 and $x$");
        assert_eq!(tokens, vec!["\\$", "5", " ", "and", " ", "$x$"]);
    }

    #[test]
    fn test_tokenize_escaped_dollar_inside_equation() {
        let tokens = tokenize_mixed("$x \\$ y$");
        assert_eq!(tokens, vec!["$x \\$ y$"]);
    }

    #[test]
    fn test_tokenize_quoted_dollar_inside_equation() {
        let tokens = tokenize_mixed("Text $\"a $ b\"$ end");
        assert_eq!(tokens, vec!["Text", " ", "$\"a $ b\"$", " ", "end"]);
    }

    #[test]
    fn test_tokenize_unclosed_dollar_is_single_token() {
        let tokens = tokenize_mixed("Use `$HOME` now");
        assert_eq!(tokens, vec!["Use", " ", "`", "$", "HOME", "`", " ", "now"]);
    }

    #[test]
    fn test_find_closing_dollar() {
        assert_eq!(find_closing_dollar("$x$ y"), Some(3));
        assert_eq!(find_closing_dollar("$a \\$ b$"), Some(8));
        assert_eq!(find_closing_dollar("$\"a $ b\"$"), Some(9));
        assert_eq!(find_closing_dollar("$\"a \\\" $ b\"$"), Some(12));
        assert_eq!(find_closing_dollar("$x"), None);
        assert_eq!(find_closing_dollar("$\"a $ b"), None);
    }

    #[test]
    fn test_tokenize_emphasis_is_atomic() {
        let tokens = tokenize_mixed("A _quick brown_ fox");
        assert_eq!(tokens, vec!["A", " ", "_quick brown_", " ", "fox"]);
    }

    #[test]
    fn test_tokenize_strong_is_atomic() {
        let tokens = tokenize_mixed("A *quick brown* fox");
        assert_eq!(tokens, vec!["A", " ", "*quick brown*", " ", "fox"]);
    }

    #[test]
    fn test_tokenize_nested_emphasis_is_one_token() {
        let tokens = tokenize_mixed("*bold _and italic_ text* end");
        assert_eq!(tokens, vec!["*bold _and italic_ text*", " ", "end"]);
    }

    #[test]
    fn test_tokenize_emphasis_with_equation_and_reference() {
        let tokens = tokenize_mixed("_the $x _ y$ value @fig_1 here_ ok");
        assert_eq!(tokens, vec!["_the $x _ y$ value @fig_1 here_", " ", "ok"]);
    }

    #[test]
    fn test_tokenize_intraword_underscore_is_not_emphasis() {
        let tokens = tokenize_mixed("snake_case and other_name");
        assert_eq!(tokens, vec!["snake_case", " ", "and", " ", "other_name"]);
    }

    #[test]
    fn test_tokenize_intraword_star_and_non_ascii() {
        assert_eq!(tokenize_mixed("2*3 é_b"), vec!["2*3", " ", "é_b"]);
        // Not intraword: a delimiter next to a space, punctuation or CJK stays separate.
        assert_eq!(tokenize_mixed("a_ b"), vec!["a", "_", " ", "b"]);
        assert_eq!(tokenize_mixed("a_猫"), vec!["a", "_", "猫"]);
    }

    /// Diff two paragraphs and return the rendered Typst markup of the result.
    fn render_diff(old: &str, new: &str) -> String {
        word_diff(old, new)
            .iter()
            .map(|s| match s.tag {
                SpanTag::Equal => s.text.clone(),
                SpanTag::Deleted => format!("#diff-deleted[{}]", s.text),
                SpanTag::Inserted => format!("#diff-added[{}]", s.text),
            })
            .collect()
    }

    /// The rendered diff must parse without errors.
    fn assert_parses(rendered: &str) {
        let root = typst_syntax::parse(rendered);
        assert!(!root.erroneous(), "does not parse: {rendered}");
    }

    #[test]
    fn test_word_diff_intraword_underscore_changes_whole_word() {
        let out = render_diff("A snake_case here.", "A camel_case here.");
        assert_eq!(
            out,
            "A #diff-deleted[snake_case]#diff-added[camel_case] here."
        );
        assert_parses(&out);
    }

    #[test]
    fn test_word_diff_intraword_underscore_variants_parse() {
        for (old, new) in [
            ("A a_b_c here.", "A a_b_d here."),
            ("A x_1 here.", "A x_2 here."),
            ("A _a_b_ c.", "A _a_d_ c."),
            ("A a*b here.", "A a*c here."),
        ] {
            let out = render_diff(old, new);
            assert_parses(&out);
        }
        assert_eq!(
            render_diff("A a_b_c here.", "A a_b_d here."),
            "A #diff-deleted[a_b_c]#diff-added[a_b_d] here."
        );
        assert_eq!(
            render_diff("A _a_b_ c.", "A _a_d_ c."),
            "A _#diff-deleted[a_b]#diff-added[a_d]_ c."
        );
    }

    #[test]
    fn test_tokenize_escaped_delimiters_are_not_emphasis() {
        let tokens = tokenize_mixed("\\_a b\\_ and \\*c d\\*");
        assert!(
            tokens
                .iter()
                .all(|t| t.len() <= 2 || !t.starts_with(['_', '*'])),
            "escaped delimiters must not form a span: {tokens:?}"
        );
        assert_eq!(&tokens[..2], &["\\_", "a"]);
    }

    #[test]
    fn test_tokenize_unclosed_emphasis_is_single_token() {
        let tokens = tokenize_mixed("A _quick brown fox");
        assert_eq!(
            tokens,
            vec!["A", " ", "_", "quick", " ", "brown", " ", "fox"]
        );
    }

    #[test]
    fn test_tokenize_delimiters_in_raw_and_function_are_not_split() {
        let tokens = tokenize_mixed("`a_b c_d` #emph[_x y_] z");
        assert_eq!(
            tokens,
            vec!["`", "a_b", " ", "c_d", "`", " ", "#emph[_x y_]", " ", "z"]
        );
    }

    #[test]
    fn test_word_diff_emphasis_deleted_whole() {
        let spans = word_diff("A _quick brown_ fox.", "A slow red fox.");
        assert!(
            spans
                .iter()
                .any(|s| s.tag == SpanTag::Deleted && s.text == "_quick brown_"),
            "emphasis should be deleted as one unit: {spans:?}"
        );
    }

    #[test]
    fn test_word_diff_emphasis_added_whole() {
        let spans = word_diff("A quick brown fox.", "A _slow red_ fox.");
        assert!(
            spans
                .iter()
                .any(|s| s.tag == SpanTag::Inserted && s.text == "_slow red_"),
            "emphasis should be added as one unit: {spans:?}"
        );
    }

    #[test]
    fn test_word_diff_kept_emphasis_diffs_inside() {
        let spans = word_diff("A _quick brown_ fox.", "A _slow brown_ fox.");
        let texts: Vec<(SpanTag, &str)> = spans.iter().map(|s| (s.tag, s.text.as_str())).collect();
        assert_eq!(
            texts,
            vec![
                (SpanTag::Equal, "A _"),
                (SpanTag::Deleted, "quick"),
                (SpanTag::Inserted, "slow"),
                (SpanTag::Equal, " brown_ fox."),
            ]
        );
    }

    #[test]
    fn test_word_diff_different_delimiters_replaced_whole() {
        let spans = word_diff("A _quick_ fox.", "A *quick* fox.");
        assert!(
            spans
                .iter()
                .any(|s| s.tag == SpanTag::Deleted && s.text == "_quick_")
                && spans
                    .iter()
                    .any(|s| s.tag == SpanTag::Inserted && s.text == "*quick*"),
            "different delimiters should be replaced whole: {spans:?}"
        );
    }

    #[test]
    fn test_tokenize_cjk_char_level() {
        // CJK characters should be tokenized individually (no whitespace boundaries).
        let tokens = tokenize_mixed("吾輩は猫である");
        assert_eq!(
            tokens,
            vec!["吾", "輩", "は", "猫", "で", "あ", "る"],
            "CJK characters should be individual tokens"
        );
    }

    #[test]
    fn test_tokenize_cjk_mixed_with_ascii() {
        // Mixed CJK and ASCII: ASCII words stay grouped, CJK splits per character.
        let tokens = tokenize_mixed("HTTP通信の実装");
        assert_eq!(tokens, vec!["HTTP", "通", "信", "の", "実", "装"],);
    }

    #[test]
    fn test_inline_footnote_produces_modified() {
        // Paragraph where @ref changed to #footnote[...] should produce Modified, not Delete+Add.
        // The paragraph must be long enough for the similarity ratio to exceed 0.5.
        let old = vec![Block::Paragraph {
            source_text: "This is a fairly long paragraph that discusses various topics in some detail. The key finding was reported by Smith et al. in their landmark study @smith_2024. Further research is needed to confirm these results across different settings and conditions.".into(),
        }];
        let new = vec![Block::Paragraph {
            source_text: "This is a fairly long paragraph that discusses various topics in some detail. The key finding was reported by Smith et al. in their landmark study #footnote[https://example.com/papers/smith2024.pdf accessed: 2025/01/15]. Further research is needed to confirm these results across different settings and conditions.".into(),
        }];

        let results = diff(&old, &new);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, DiffResult::Modified { .. })),
            "expected Modified for inline footnote change, got: {results:?}"
        );
    }

    #[test]
    fn test_cjk_word_diff_char_granularity() {
        // CJK text with a small change should produce character-level Modified, not whole-block Delete+Add.
        let old = vec![Block::Paragraph {
            source_text: "本手法では、入力データを前処理した後、特徴量を抽出する。抽出された特徴量に基づき分類を行う。".into(),
        }];
        let new = vec![Block::Paragraph {
            source_text: "本手法では、入力データを前処理した後、特徴ベクトルを抽出する。抽出された特徴ベクトルに基づき分類を行う。".into(),
        }];
        let results = diff(&old, &new);
        if let DiffResult::Modified { spans, .. } = &results[0] {
            assert!(
                spans.iter().any(|s| s.tag == SpanTag::Equal),
                "CJK diff should have Equal spans (character-level granularity), got: {spans:?}"
            );
            assert!(
                spans
                    .iter()
                    .any(|s| s.tag == SpanTag::Deleted && s.text.contains("量")),
                "should delete old CJK characters"
            );
            assert!(
                spans
                    .iter()
                    .any(|s| s.tag == SpanTag::Inserted && s.text.contains("ベクトル")),
                "should insert new CJK characters"
            );
        } else {
            panic!("expected Modified, got: {:?}", results[0]);
        }
    }

    #[test]
    fn test_coalesce_spans() {
        let spans = vec![
            DiffSpan {
                tag: SpanTag::Equal,
                text: "a".into(),
            },
            DiffSpan {
                tag: SpanTag::Equal,
                text: "b".into(),
            },
            DiffSpan {
                tag: SpanTag::Deleted,
                text: "c".into(),
            },
        ];
        let result = coalesce_spans(spans);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "ab");
        assert_eq!(result[1].text, "c");
    }
}
