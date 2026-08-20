use typdiff::diff::diff;
use typdiff::parse::parse;
use typdiff::render::render;

fn run_diff(old: &str, new: &str) -> String {
    let old_blocks: Vec<_> = parse(old)
        .into_iter()
        .filter(|b| !matches!(b, typdiff::Block::Parbreak))
        .collect();
    let new_blocks: Vec<_> = parse(new)
        .into_iter()
        .filter(|b| !matches!(b, typdiff::Block::Parbreak))
        .collect();
    let results = diff(&old_blocks, &new_blocks);
    render(&results)
}

#[test]
fn test_identical_documents() {
    let src = "= Title\n\nHello world.\n";
    let output = run_diff(src, src);
    assert!(output.contains("= Title"));
    assert!(output.contains("Hello world."));
    // The preamble defines diff-added/diff-deleted, but the body should not use them.
    assert!(!output.contains("#diff-added["));
    assert!(!output.contains("#diff-deleted["));
}

#[test]
fn test_heading_change() {
    let old = "= Introduction\n\nSome text.\n";
    let new = "= Background\n\nSome text.\n";
    let output = run_diff(old, new);
    assert!(output.contains("diff-deleted"));
    assert!(output.contains("diff-added"));
    assert!(output.contains("Some text."));
}

#[test]
fn test_paragraph_word_change() {
    let old = "= Title\n\nThis is the old text.\n";
    let new = "= Title\n\nThis is the new text.\n";
    let output = run_diff(old, new);
    // Title should be unchanged.
    assert!(output.contains("= Title"));
    // "old" should be deleted, "new" should be added.
    assert!(output.contains("#diff-deleted[old]"));
    assert!(output.contains("#diff-added[new]"));
}

#[test]
fn test_added_paragraph() {
    let old = "= Title\n";
    let new = "= Title\n\nNew paragraph here.\n";
    let output = run_diff(old, new);
    assert!(output.contains("diff-added"));
}

#[test]
fn test_deleted_paragraph() {
    let old = "= Title\n\nOld paragraph here.\n";
    let new = "= Title\n";
    let output = run_diff(old, new);
    assert!(output.contains("diff-deleted"));
}

#[test]
fn test_list_items() {
    let old = "- Apple\n- Banana\n";
    let new = "- Apple\n- Cherry\n";
    let output = run_diff(old, new);
    assert!(output.contains("Apple"));
    // Banana should be marked as deleted or modified, Cherry as added.
    assert!(output.contains("diff-deleted") || output.contains("diff-added"));
}

#[test]
fn test_empty_to_content() {
    let old = "";
    let new = "= New Document\n\nContent here.\n";
    let output = run_diff(old, new);
    assert!(output.contains("diff-added"));
}

#[test]
fn test_content_to_empty() {
    let old = "= Old Document\n\nContent here.\n";
    let new = "";
    let output = run_diff(old, new);
    assert!(output.contains("diff-deleted"));
}

#[test]
fn test_output_contains_preamble() {
    let output = run_diff("Hello\n", "World\n");
    assert!(output.contains("#let diff-added(body)"));
    assert!(output.contains("#let diff-deleted(body)"));
    assert!(output.contains("underline"));
    assert!(output.contains("strike"));
}

#[test]
fn test_multiline_paragraph_change() {
    let old = "First line and second part.\n";
    let new = "First line and third part.\n";
    let output = run_diff(old, new);
    assert!(output.contains("#diff-deleted[second]"));
    assert!(output.contains("#diff-added[third]"));
}

#[test]
fn test_inline_footnote_ref_to_funccall() {
    // When @ref changes to #footnote[...] in a long paragraph, it should produce
    // Modified with word-level diff. The paragraph must be long enough for
    // the similarity ratio to exceed 0.5.
    let old = "This is a fairly long paragraph that discusses various topics in some detail. The key finding was reported by Smith et al. in their landmark study @smith_2024. Further research is needed to confirm these results across different settings and conditions.\n\nAnother paragraph.\n";
    let new = "This is a fairly long paragraph that discusses various topics in some detail. The key finding was reported by Smith et al. in their landmark study #footnote[https://example.com/papers/smith2024.pdf accessed: 2025/01/15]. Further research is needed to confirm these results across different settings and conditions.\n\nAnother paragraph.\n";
    let output = run_diff(old, new);
    // The surrounding text should be inline with diff spans (Modified), not whole-block Delete+Add
    assert!(
        output.contains("study #diff-deleted["),
        "surrounding text should be inline with diff spans: {output}"
    );
}

#[test]
fn test_markup_reference_paragraph() {
    let old = std::fs::read_to_string("tests/fixtures/markup-ref-start-old.typ").unwrap();
    let new = std::fs::read_to_string("tests/fixtures/markup-ref-start-new.typ").unwrap();
    let output = run_diff(&old, &new);

    // the diff should remain a single paragraph with no blank line inserted.
    assert!(
        !output.contains("#[@foo]\n\n"),
        "unexpected blank line: {output}"
    );
    assert!(output.contains("#[@foo]のような"));
}

#[test]
fn test_renamed_label_is_not_diffed_inside_label_syntax() {
    let old = "= Sample\n\n<sample-widget-anchor>\n\nBody.\n";
    let new = "= Sample\n\n<sample-widget_anchor>\n\nBody.\n";
    let output = run_diff(old, new);

    assert!(output.contains("<sample-widget_anchor>"));
    assert!(!output.contains("<#diff-added"));
    assert!(!output.contains("#diff-added[_]"));
    assert!(!output.contains("#diff-deleted[-]"));
}

#[test]
fn test_unchanged_label_stays_outside_added_paragraph() {
    let old = r#"#set heading(numbering: "1.")

See #ref(<sample-anchor>, supplement: [Section]).

= Sample

<sample-anchor>
Alpha beta gamma delta epsilon zeta eta theta.
"#;
    let new = r#"#set heading(numbering: "1.")

See #ref(<sample-anchor>, supplement: [Section]).

= Sample

<sample-anchor>
Inserted paragraph before old text.

Alpha beta gamma delta changed epsilon zeta eta theta.
"#;
    let output = run_diff(old, new);

    assert!(output.contains("<sample-anchor>"));
    assert!(output.contains("#diff-added[Inserted paragraph before old text.]"));
    assert!(output.contains("Alpha beta gamma delta #diff-added[changed ]epsilon"));
    assert!(!output.contains("#diff-added[<sample-anchor>"));
    assert!(!output.contains("#diff-deleted[\\<sample-anchor>"));
}

#[test]
fn test_replaced_footnote_ref_label_is_not_escaped_on_deleted_side() {
    let old = r#"The bridge inspection memo kept its summary sentence for editors#footnote[
Earlier notes pointed reviewers to #ref(<sec-bridge-ledger>, supplement: [])
while the field log was being reconciled.
]. The closing sentence stays fixed so the paragraph can align.
"#;
    let new = r#"The bridge inspection memo kept its summary sentence for editors#footnote[
Current notes point reviewers to #ref(<sec-bridge-ledger>, supplement: [])
after the field log was reconciled.
]. The closing sentence stays fixed so the paragraph can align.
"#;
    let output = run_diff(old, new);

    assert!(output.contains("#diff-deleted[#footnote["));
    assert!(output.contains("#ref(<sec-bridge-ledger>, supplement: [])"));
    assert!(!output.contains("#ref(\\<sec-bridge-ledger>"));
}

#[test]
fn test_indented_line_comment_does_not_split_paragraph() {
    let old = "First sentence.\n  // indented comment\nSecond sentence.\n";
    let new = "First sentence.\n  // indented comment\nSecond sentence changed.\n";
    let output = run_diff(old, new);

    assert!(output.contains("First sentence.\nSecond sentence"));
    // A whitespace-only line would be treated as a paragraph break by Typst.
    assert!(!output.contains("\n  \n"));
}
