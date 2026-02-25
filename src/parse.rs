use typst_syntax::ast::{self, AstNode, Expr};

use crate::Block;

/// Parse a Typst source string into a sequence of blocks.
pub fn parse(source: &str) -> Vec<Block> {
    let root = typst_syntax::parse(source);
    let markup: ast::Markup = root.cast().expect("failed to parse as markup");

    let mut blocks = Vec::new();
    let mut paragraph_buf = String::new();

    for expr in markup.exprs() {
        match expr {
            // ---- block-level elements ----
            Expr::Heading(h) => {
                flush_paragraph(&mut paragraph_buf, &mut blocks);
                let depth = h.depth().get();
                let body_text = markup_text(h.body());
                blocks.push(Block::Heading { depth, body_text });
            }
            Expr::ListItem(li) => {
                flush_paragraph(&mut paragraph_buf, &mut blocks);
                let body_text = markup_text(li.body());
                blocks.push(Block::ListItem { body_text });
            }
            Expr::EnumItem(ei) => {
                flush_paragraph(&mut paragraph_buf, &mut blocks);
                let number = ei.number().map(|n| n as usize);
                let body_text = markup_text(ei.body());
                blocks.push(Block::EnumItem { number, body_text });
            }
            Expr::TermItem(ti) => {
                flush_paragraph(&mut paragraph_buf, &mut blocks);
                let term = markup_text(ti.term());
                let description = markup_text(ti.description());
                blocks.push(Block::TermItem { term, description });
            }
            Expr::Parbreak(_) => {
                flush_paragraph(&mut paragraph_buf, &mut blocks);
                // Collapse consecutive Parbreaks into one. Extra blank lines
                // (e.g. around /* textlint-disable */ comments) are semantically
                // equivalent to a single paragraph break.
                if !matches!(blocks.last(), Some(Block::Parbreak)) {
                    blocks.push(Block::Parbreak);
                }
            }
            Expr::Raw(r) if r.block() => {
                flush_paragraph(&mut paragraph_buf, &mut blocks);
                blocks.push(Block::RawBlock {
                    content: node_text(&expr),
                });
            }
            Expr::Equation(eq) if eq.block() => {
                flush_paragraph(&mut paragraph_buf, &mut blocks);
                blocks.push(Block::Equation {
                    block: true,
                    content: node_text(&expr),
                });
            }

            // ---- inline elements (accumulate into paragraph) ----
            Expr::Text(_)
            | Expr::Space(_)
            | Expr::Linebreak(_)
            | Expr::Escape(_)
            | Expr::Shorthand(_)
            | Expr::SmartQuote(_)
            | Expr::Strong(_)
            | Expr::Emph(_)
            | Expr::Link(_)
            | Expr::Label(_)
            | Expr::Ref(_) => {
                paragraph_buf.push_str(&node_text(&expr));
            }
            // inline raw
            Expr::Raw(_) => {
                paragraph_buf.push_str(&node_text(&expr));
            }
            // inline equation
            Expr::Equation(_) => {
                paragraph_buf.push_str(&node_text(&expr));
            }

            // ---- everything else: FuncCall, CodeBlock, etc. ----
            // If we are mid-paragraph, keep the expression inline so it
            // doesn't break the paragraph into fragments (e.g. #footnote[…]
            // between sentences). Otherwise treat it as an atomic block.
            _ => {
                if paragraph_buf.trim().is_empty() {
                    flush_paragraph(&mut paragraph_buf, &mut blocks);
                    blocks.push(Block::FuncCall {
                        content: node_text(&expr),
                    });
                } else {
                    paragraph_buf.push_str(&node_text(&expr));
                }
            }
        }
    }

    flush_paragraph(&mut paragraph_buf, &mut blocks);
    blocks
}

/// Extract the source text of a Markup body, trimming leading whitespace.
fn markup_text(markup: ast::Markup<'_>) -> String {
    let text = markup.to_untyped().clone().into_text().to_string();
    text.trim_start().to_string()
}

/// Extract the full source text of any AST expression.
///
/// Code-mode expressions (FuncCall, SetRule, ShowRule, etc.) need a `#` prefix
/// when they appear in markup, because the AST node does not include it.
fn node_text(expr: &Expr<'_>) -> String {
    let text = expr.to_untyped().clone().into_text().to_string();
    if expr.hash() {
        format!("#{text}")
    } else {
        text
    }
}

/// Flush the paragraph buffer into blocks if non-empty.
fn flush_paragraph(buf: &mut String, blocks: &mut Vec<Block>) {
    let trimmed = buf.trim();
    if !trimmed.is_empty() {
        blocks.push(Block::Paragraph {
            source_text: trimmed.to_string(),
        });
    }
    buf.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_heading() {
        let blocks = parse("= Introduction\n");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], Block::Heading { depth: 1, body_text } if body_text == "Introduction")
        );
    }

    #[test]
    fn test_parse_paragraph() {
        let blocks = parse("Hello world.\n");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], Block::Paragraph { source_text } if source_text == "Hello world.")
        );
    }

    #[test]
    fn test_parse_heading_and_paragraph() {
        let blocks = parse("= Title\n\nSome text here.\n");
        assert!(blocks.len() >= 3);
        assert!(matches!(&blocks[0], Block::Heading { depth: 1, .. }));
        assert!(matches!(&blocks[1], Block::Parbreak));
        assert!(matches!(&blocks[2], Block::Paragraph { .. }));
    }

    #[test]
    fn test_parse_list_items() {
        let blocks = parse("- First\n- Second\n");
        assert!(blocks.iter().any(|b| matches!(b, Block::ListItem { .. })));
    }

    #[test]
    fn test_parse_enum_items() {
        let blocks = parse("+ One\n+ Two\n");
        assert!(blocks.iter().any(|b| matches!(b, Block::EnumItem { .. })));
    }

    #[test]
    fn test_parse_inline_markup() {
        let blocks = parse("Hello *bold* and _italic_ text.\n");
        assert_eq!(blocks.len(), 1);
        if let Block::Paragraph { source_text } = &blocks[0] {
            assert!(source_text.contains("*bold*"));
            assert!(source_text.contains("_italic_"));
        } else {
            panic!("expected paragraph");
        }
    }

    #[test]
    fn test_parse_inline_funccall() {
        // #footnote[...] mid-paragraph should stay inline, not split the paragraph.
        let blocks = parse("Some text before #footnote[https://example.com] and after.\n");
        assert_eq!(blocks.len(), 1, "expected 1 block, got: {blocks:?}");
        if let Block::Paragraph { source_text } = &blocks[0] {
            assert!(source_text.contains("#footnote["));
            assert!(source_text.contains("Some text before"));
            assert!(source_text.contains("and after."));
        } else {
            panic!("expected paragraph, got: {:?}", blocks[0]);
        }
    }

    #[test]
    fn test_parse_markup_reference_split() {
        let text = "#[@foo]が段落の先頭にある場合、改行が挿入されないことを確認するテストです。\n";
        let blocks = parse(text);
        assert_eq!(blocks.len(), 1, "expected single paragraph block");
        if let Block::Paragraph { source_text } = &blocks[0] {
            assert!(source_text.starts_with("#[@foo]"));
        }
    }

    #[test]
    fn test_parse_contentblock_various_positions() {
        for &case in &["#[foo]\n", "prefix #[foo] suffix\n", "#[foo]#[bar]\n"] {
            let blocks = parse(case);
            assert_eq!(blocks.len(), 1);
            if let Block::Paragraph { source_text } = &blocks[0] {
                assert!(source_text.contains("#[foo]"));
            }
        }
    }
}
