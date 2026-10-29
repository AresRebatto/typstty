//! Regression tests for the items of typst-live-report.md that live in the model.
use typst_live::commands::{Command::*, CommandContext, apply, apply_ctx};
use typst_live::doc::*;
use typst_live::engine::*;

fn style() -> RenderStyle {
    RenderStyle {
        width_pt: 400.0,
        font_pt: 12.0,
        line_spacing: 1.4,
        dark: false,
        pixels_per_pt: 1.0,
    }
}

// ---------- U1 / E9: merging blocks ----------
#[test]
fn backspace_at_block_start_merges_with_previous_block() {
    let t = "A\n\nB";
    let b = segment(t);
    let m = backspace_at_block_start(t, &b, b[1].range.start).unwrap();
    assert_eq!(m.text, "A B");
    assert_eq!(m.caret, 1); // right after "A"
    assert_eq!(segment(&m.text).len(), 1);
}

#[test]
fn delete_at_block_end_is_symmetric() {
    let t = "A\n\nB";
    let b = segment(t);
    let m = delete_at_block_end(t, &b, b[0].range.end).unwrap();
    assert_eq!(m.text, "A B");
    assert_eq!(m.caret, 1);
}

#[test]
fn merge_never_fuses_a_heading_with_a_paragraph() {
    let t = "= H\n\nbody";
    let b = segment(t);
    let m = backspace_at_block_start(t, &b, b[1].range.start).unwrap();
    assert_eq!(m.text, "= H\nbody");
    assert_eq!(m.caret, 3);
    assert_eq!(segment(&m.text).len(), 2, "still two blocks");
}

#[test]
fn merge_is_one_undo_step_restoring_text_and_caret() {
    let mut d = Document::new("A\n\nB".into());
    d.set_selection(3, 3);
    d.commit(true);
    let b = d.blocks();
    let m = backspace_at_block_start(&d.text, &b, 3).unwrap();
    d.apply_merge(m);
    assert_eq!((d.text.as_str(), d.sel()), ("A B", 1..1));
    assert!(d.undo());
    assert_eq!(d.text, "A\n\nB");
    assert_eq!(d.sel(), 3..3);
}

#[test]
fn backspace_in_first_block_only_removes_leading_whitespace() {
    let t = "\n\nA";
    let b = segment(t);
    let m = backspace_at_block_start(t, &b, 2).unwrap();
    assert_eq!(m.text, "A");
    assert!(backspace_at_block_start("A", &segment("A"), 0).is_none());
}

// ---------- U4 / E10 / D3: document-level selection ----------
#[test]
fn select_all_twice_selects_the_whole_document() {
    let mut d = Document::new("one\n\ntwo\n\nthree".into());
    d.set_selection(6, 6);
    let blocks = d.blocks();
    let active = active_range(&d.text, &blocks, 6);
    d.select_all_step(active.clone());
    assert_eq!(d.sel(), active.clone(), "first press = block");
    assert!(!d.multi);
    d.select_all_step(active);
    assert_eq!(d.sel(), 0..d.text.len(), "second press = document");
    assert!(d.multi);
}

#[test]
fn typing_over_a_whole_document_selection_replaces_everything_and_undo_restores() {
    let mut d = Document::new("one\n\ntwo\n\nthree".into());
    let orig = d.text.clone();
    d.set_selection(0, 0);
    d.commit(true);
    d.set_multi_selection(0, d.text.len());
    d.replace_selection("x");
    assert_eq!(d.text, "x");
    assert!(!d.multi);
    assert!(d.undo());
    assert_eq!(d.text, orig);
}

#[test]
fn moving_the_caret_leaves_document_selection_mode() {
    let mut d = Document::new("a\n\nb".into());
    d.set_multi_selection(0, 4);
    assert!(d.multi);
    d.set_selection(1, 1);
    assert!(!d.multi);
}

#[test]
fn head_movement_across_blocks() {
    let t = "aa\n\nbb\n\ncc";
    let b = segment(t);
    // blocks: aa 0..2, bb 4..6, cc 8..10
    assert_eq!(move_head(t, &b, 4, Dir::Up), 0);
    assert_eq!(move_head(t, &b, 7, Dir::Up), 4);
    assert_eq!(move_head(t, &b, 4, Dir::Down), 6);
    assert_eq!(move_head(t, &b, 6, Dir::Down), 10);
    assert_eq!(move_head(t, &b, 0, Dir::DocEnd), 10);
    assert_eq!(move_head(t, &b, 5, Dir::DocStart), 0);
    assert!(within_one_block(&b, 4, 6));
    assert!(!within_one_block(&b, 1, 5));
    // multibyte safe
    assert_eq!(move_head("é", &segment("é"), 2, Dir::Left), 0);
}

// ---------- E1: block markers ----------
#[test]
fn bold_keeps_list_and_heading_markers_outside_the_delimiters() {
    let t = "- one\n- two";
    let e = apply(Bold, t, 0, t.len());
    assert_eq!(e.text, "- *one*\n- *two*");
    let e2 = apply(Bold, &e.text, e.start, e.end);
    assert_eq!(e2.text, t, "toggle back");

    let t = "= Titolo";
    let e = apply(Bold, t, 0, t.len());
    assert_eq!(e.text, "= *Titolo*");

    let t = "+ a\n+ b";
    assert_eq!(apply(Italic, t, 0, t.len()).text, "+ _a_\n+ _b_");
}

// ---------- E2: intra-word emphasis ----------
#[test]
fn intra_word_emphasis_uses_the_functional_form() {
    let e = apply(Bold, "abcdef", 2, 4);
    assert_eq!(e.text, "ab#strong[cd]ef");
    assert_eq!(&e.text[e.start..e.end], "cd");
    let back = apply(Bold, &e.text, e.start, e.end);
    assert_eq!(back.text, "abcdef");
    assert_eq!(apply(Italic, "abcdef", 2, 4).text, "ab#emph[cd]ef");
    // whole word keeps the markup form
    assert_eq!(apply(Bold, "x abc y", 2, 5).text, "x *abc* y");
    // and the produced source really bolds in Typst
    assert!(render_block(&e.text, "", &style(), None).is_ok());
}

// ---------- E3: code language survives unwrap/re-wrap ----------
#[test]
fn code_block_language_is_restored_when_rewrapping() {
    let mut ctx = CommandContext::default();
    let t = "```rust\nfn a() {}\n```";
    let un = apply_ctx(CodeBlock, t, 2, 2, &mut ctx);
    assert_eq!(un.text, "fn a() {}");
    let re = apply_ctx(CodeBlock, &un.text, un.start, un.end, &mut ctx);
    assert_eq!(re.text, t);
    // an unrelated body does not inherit the tag
    let other = apply_ctx(CodeBlock, "something else", 0, 4, &mut ctx);
    assert!(other.text.starts_with("```\n"));
}

// ---------- E4: multi-line math indentation ----------
#[test]
fn display_math_multiline_is_reindented() {
    let t = "  q\n  w";
    let e = apply(DisplayMath, t, 2, t.len());
    assert_eq!(e.text, "  $\n    q\n    w\n  $");
    assert!(segment(&e.text).len() == 1);
}

// ---------- E7: CRLF / BOM ----------
#[test]
fn crlf_and_bom_round_trip_byte_for_byte() {
    let raw = "\u{feff}= T\r\n\r\npara\r\n";
    let mut d = Document::new(String::new());
    d.load_raw(raw);
    assert_eq!(d.text, "= T\n\npara\n");
    assert_eq!(segment(&d.text).len(), 2);
    assert_eq!(d.disk_bytes(), raw.as_bytes());
    d.load_raw("plain\n");
    assert_eq!(d.disk_bytes(), b"plain\n");
}

// ---------- F1: atomic save ----------
#[test]
fn atomic_write_replaces_content_and_leaves_no_temp_files() {
    let dir = std::env::temp_dir().join(format!("typst-live-atomic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.typ");
    atomic_write(&p, b"old").unwrap();
    atomic_write(&p, b"new content").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"new content");
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["a.typ"], "{names:?}");
    // failure leaves the original untouched
    assert!(atomic_write(&dir.join("missing-dir/x.typ"), b"z").is_err());
}

// ---------- P1: cached segmentation, O(1) dirty ----------
#[test]
fn block_cache_follows_edits_and_dirty_flag_is_exact() {
    let mut d = Document::new("a\n\nb".into());
    assert_eq!(d.blocks().len(), 2);
    assert!(!d.is_dirty());
    d.replace_range(4..4, "\n\nc", true);
    assert_eq!(d.blocks().len(), 3, "cache invalidated by the edit");
    assert!(d.is_dirty());
    assert!(d.undo());
    assert_eq!(d.blocks().len(), 2);
    assert!(!d.is_dirty(), "undo back to the saved text is clean");
    d.mark_saved();
    d.load("z".into());
    assert!(!d.is_dirty());
}

// ---------- C1: every page is shown ----------
#[test]
fn multi_page_block_is_stacked_not_truncated() {
    let one = render_block("a", "", &style(), None).unwrap();
    let two = render_block("a\n#pagebreak()\nb", "", &style(), None).unwrap();
    assert_eq!(one.pages, 1);
    assert_eq!(two.pages, 2);
    assert!(two.height > one.height, "{} vs {}", two.height, one.height);
    assert!(!two.truncated);
    assert_eq!(two.rgba.len(), two.width * two.height * 4);
}

// ---------- C5: diagnostics of imported files ----------
#[test]
fn errors_in_imported_files_are_not_mapped_into_the_main_document() {
    let dir = std::env::temp_dir().join(format!("typst-live-diag-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.typ"), "#let x = (\n").unwrap();
    let main = "= Title\n\n#import \"bad.typ\": x\n\nbody\n";
    let r = compile_full(main, Some(dir), false);
    assert!(r.diags.iter().any(|d| d.error), "{:?}", r.diags);
    for d in &r.diags {
        if d.file.is_some() {
            assert!(d.range.is_none() && d.line.is_none(), "{d:?}");
        }
    }
    assert!(
        r.diags
            .iter()
            .any(|d| d.file.as_deref() == Some("/bad.typ")),
        "{:?}",
        r.diags
    );
}

// ---------- P5: safe wrappers ----------
#[test]
fn safe_wrappers_behave_like_the_plain_functions() {
    assert!(render_block_safe("= ok", "", &style(), None).is_ok());
    assert!(render_block_safe("#let x = (", "", &style(), None).is_err());
    assert!(compile_full_safe("= ok", None, true).pdf.is_some());
}
