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

#[test]
fn segmentation_covers_source_and_splits_headings_and_paragraphs() {
    let t =
        "= Title\nPara one\nstill one\n\nPara two\n\n#set text(size: 9pt)\n\n$ x^2 $\n\n- a\n- b\n";
    let b = segment(t);
    let texts: Vec<&str> = b.iter().map(|b| &t[b.range.clone()]).collect();
    assert_eq!(texts[0], "= Title");
    assert_eq!(texts[1], "Para one\nstill one");
    assert_eq!(texts[2], "Para two");
    assert_eq!(texts[3], "#set text(size: 9pt)");
    assert_eq!(b[3].kind, BlockKind::Definition);
    assert_eq!(texts[4], "$ x^2 $");
    assert_eq!(texts[5], "- a\n- b");
    // everything between blocks is whitespace → nothing can be lost
    let mut pos = 0;
    for blk in &b {
        assert!(t[pos..blk.range.start].trim().is_empty());
        pos = blk.range.end;
    }
    assert!(t[pos..].trim().is_empty());
}

#[test]
fn multiline_raw_and_equation_with_blank_lines_stay_whole() {
    let t = "```rust\nfn a() {}\n\nfn b() {}\n```\n\n$\n  a\n\n  b\n$\n";
    let b = segment(t);
    assert_eq!(
        &t[b[0].range.clone()],
        "```rust\nfn a() {}\n\nfn b() {}\n```"
    );
}

#[test]
fn malformed_source_still_segments() {
    for t in ["*oops", "$ x", "```\nopen", "#let (", "= \n", ""] {
        let _ = segment(t);
    }
}

#[test]
fn active_range_stays_in_block_after_one_newline_and_splits_after_two() {
    let t = "- a\n";
    let b = segment(t);
    assert_eq!(active_range(t, &b, 4), 0..4);
    let t = "para\n\n";
    let b = segment(t);
    assert_eq!(active_range(t, &b, 6), 6..6);
}

#[test]
fn real_typst_renders_heading_math_code_table() {
    for src in [
        "= Heading",
        "Hello *bold* _it_ `code` #link(\"https://typst.app\")[x]",
        "$ x^2 + y^2 = z^2 $",
        "```rust\nfn main() {}\n```",
        "#table(columns: 2, [a], [b], [c], [d])",
    ] {
        let r = render_block(src, "", &style(), None).unwrap_or_else(|d| panic!("{src}: {d:?}"));
        assert!(r.width > 10 && r.height > 5, "{src}");
        assert!(r.rgba.chunks(4).any(|p| p[3] > 0), "{src} rendered blank");
    }
}

#[test]
fn definitions_apply_to_later_blocks() {
    let plain = render_block("Big", "", &style(), None).unwrap();
    let big = render_block("Big", "#set text(size: 30pt)\n", &style(), None).unwrap();
    assert!(big.height > plain.height);
}

#[test]
fn invalid_syntax_reports_diagnostics_and_recovers() {
    let e = render_block("#let x = (", "", &style(), None)
        .err()
        .expect("should fail");
    assert!(e.iter().any(|d| d.error));
    let ok = render_block("= Fixed", "", &style(), None);
    assert!(ok.is_ok());
    let full = compile_full("= A\n\n#unknown()\n", None, false);
    assert!(
        full.diags.iter().any(|d| d.error && d.line == Some(3)),
        "{:?}",
        full.diags
    );
}

#[test]
fn pdf_export_uses_the_real_compiler() {
    let r = compile_full("= Hi\n\n$ a + b $", None, true);
    let pdf = r.pdf.expect("pdf");
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn undo_redo_restore_text_and_selection() {
    let mut d = Document::new(String::new());
    d.text = "abc".into();
    d.set_selection(3, 3);
    d.commit(true);
    let e = typst_live::commands::apply(typst_live::commands::Command::Bold, &d.text.clone(), 0, 3);
    d.apply_edit(e);
    assert_eq!(d.text, "*abc*");
    assert_eq!(d.sel(), 1..4);
    assert!(d.undo());
    assert_eq!(d.text, "abc");
    assert_eq!(d.sel(), 3..3);
    assert!(d.redo());
    assert_eq!(d.text, "*abc*");
    assert_eq!(d.sel(), 1..4);
}

#[test]
fn typing_bursts_coalesce_into_one_undo_step() {
    let mut d = Document::new(String::new());
    for (i, ch) in "hello".chars().enumerate() {
        d.replace_range(i..i, &ch.to_string(), false);
        d.set_selection(i + 1, i + 1);
    }
    assert!(d.is_dirty());
    assert!(d.undo());
    assert_eq!(d.text, "");
}

#[test]
fn large_document_and_rapid_typing() {
    let mut t = String::new();
    for i in 0..3000 {
        t.push_str(&format!(
            "= Section {i}\n\nParagraph *{i}* with $x_{i}$ math.\n\n"
        ));
    }
    let start = std::time::Instant::now();
    let b = segment(&t);
    assert_eq!(b.len(), 6000);
    assert!(
        start.elapsed().as_secs() < 5,
        "segmentation too slow: {:?}",
        start.elapsed()
    );
    let mut d = Document::new(t);
    let start = std::time::Instant::now();
    for i in 0..500 {
        let at = d.text.len();
        d.replace_range(at..at, "x", false);
        d.set_selection(at + 1, at + 1);
        if i % 50 == 0 {
            let _ = d.blocks();
        }
    }
    assert!(start.elapsed().as_secs() < 20);
}
