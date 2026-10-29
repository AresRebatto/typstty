//! End-to-end (model + real compiler + files): type Typst, see it rendered,
//! put the caret in the formatted content, edit it, save the .typ file.
use typst_live::commands::{Command, apply};
use typst_live::doc::*;
use typst_live::engine::*;

#[test]
fn type_render_edit_save_reopen() {
    let style = RenderStyle {
        width_pt: 500.0,
        font_pt: 12.0,
        line_spacing: 1.4,
        dark: false,
        pixels_per_pt: 1.0,
    };
    let mut d = Document::new(String::new());

    // 1. "type" source
    let src = "= Notes\n\nEuler: $e^(i pi) + 1 = 0$ and some text.\n";
    d.replace_range(0..0, src, true);
    d.set_selection(src.len(), src.len());

    // 2. every block renders to formatted pixels
    let blocks = d.blocks();
    assert_eq!(blocks.len(), 2);
    for b in &blocks {
        let r = render_block(&d.text[b.range.clone()], "", &style, None).unwrap();
        assert!(r.rgba.chunks(4).any(|p| p[3] > 0));
    }

    // 3. caret enters the paragraph → its source is the active range
    let caret = d.text.find("some").unwrap();
    d.set_selection(caret, caret);
    let active = active_range(&d.text, &d.blocks(), caret);
    assert_eq!(
        &d.text[active.clone()],
        "Euler: $e^(i pi) + 1 = 0$ and some text."
    );

    // 4. modify through a shortcut: bold the word "some"
    let e = apply(Command::Bold, &d.text.clone(), caret, caret + 4);
    d.apply_edit(e);
    assert!(d.text.contains("*some*"));
    assert_eq!(&d.text[d.sel()], "some");

    // 5. re-render the edited block, no error
    let blocks = d.blocks();
    assert!(render_block(&d.text[blocks[1].range.clone()], "", &style, None).is_ok());

    // 6. save → bytes on disk are exactly the source; reopen is lossless
    let dir = std::env::temp_dir().join(format!("typst-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.typ");
    std::fs::write(&path, &d.text).unwrap();
    d.mark_saved();
    assert!(!d.is_dirty());
    let reopened = std::fs::read_to_string(&path).unwrap();
    assert_eq!(reopened, d.text);
    assert_eq!(
        reopened,
        "= Notes\n\nEuler: $e^(i pi) + 1 = 0$ and *some* text.\n"
    );

    // 7. undo restores content and selection
    assert!(d.undo());
    assert!(d.text.contains("and some text"));
    assert!(d.is_dirty());
}

#[test]
fn unsupported_and_unknown_syntax_is_preserved_verbatim() {
    let weird = "#import \"missing.typ\": *\n\n#let f(x) = x + 1\n\n@nolabel and #unknown()\n\n```\nunterminated";
    let mut d = Document::new(weird.to_string());
    for b in d.blocks() {
        let _ = render_block(
            &d.text[b.range.clone()],
            "",
            &RenderStyle {
                width_pt: 400.0,
                font_pt: 12.0,
                line_spacing: 1.4,
                dark: false,
                pixels_per_pt: 1.0,
            },
            None,
        );
    }
    d.commit(true);
    assert_eq!(d.text, weird);
}
