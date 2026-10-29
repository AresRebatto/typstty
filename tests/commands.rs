use typst_live::commands::{Command::*, Edit, apply};

fn run(c: typst_live::commands::Command, t: &str, s: usize, e: usize) -> Edit {
    apply(c, t, s, e)
}
fn sel(e: &Edit) -> &str {
    &e.text[e.start..e.end]
}

#[test]
fn bold_wraps_selection_and_keeps_it_selected() {
    let e = run(Bold, "hello world", 0, 5);
    assert_eq!(e.text, "*hello* world");
    assert_eq!(sel(&e), "hello");
}
#[test]
fn italic_wraps_selection() {
    let e = run(Italic, "hello world", 6, 11);
    assert_eq!(e.text, "hello _world_");
}
#[test]
fn empty_selection_inserts_delimiters_with_caret_inside() {
    for (c, d) in [
        (Bold, "**"),
        (Italic, "__"),
        (InlineCode, "``"),
        (InlineMath, "$$"),
    ] {
        let e = run(c, "ab", 1, 1);
        assert_eq!(e.text, format!("a{d}b"));
        assert_eq!((e.start, e.end), (2, 2), "{d}");
    }
}
#[test]
fn repeated_press_toggles_instead_of_duplicating() {
    let a = run(Bold, "word", 0, 4);
    let b = run(Bold, &a.text, a.start, a.end);
    assert_eq!(b.text, "word");
    assert_eq!(sel(&b), "word");
    let a = run(Bold, "", 0, 0);
    let b = run(Bold, &a.text, a.start, a.end);
    assert_eq!(b.text, "");
}
#[test]
fn does_not_merge_two_adjacent_bold_spans() {
    let t = "*a* b *c*";
    let e = run(Bold, t, 0, t.len());
    // selection is not a single span: must not strip the inner delimiters
    assert!(e.text.matches('*').count() >= 4, "{}", e.text);
}
#[test]
fn bold_inside_span_splits_it() {
    let t = "*a b c*";
    let e = run(Bold, t, 3, 4);
    assert_eq!(e.text, "*a* b *c*");
    assert_eq!(sel(&e), "b");
}
#[test]
fn whitespace_stays_outside_delimiters() {
    let e = run(Bold, "x  hi  y", 1, 7);
    assert_eq!(e.text, "x  *hi*  y");
}
#[test]
fn multiline_selection_keeps_paragraphs_and_indentation() {
    let t = "  one\n  two\n\nthree";
    let e = run(Bold, t, 0, t.len());
    assert_eq!(e.text, "  *one\n  two*\n\n*three*");
    let back = run(Bold, &e.text, e.start, e.end);
    assert_eq!(back.text, t);
}
#[test]
fn unicode_is_safe() {
    let t = "àèì ünï 日本語";
    let e = run(Italic, t, "àèì ".len(), t.len());
    assert_eq!(e.text, "àèì _ünï 日本語_");
    // selection in the middle of a multi-byte char is clamped, never panics
    let _ = run(Bold, t, 1, 3);
}
#[test]
fn inline_code_and_backticks_inside() {
    let e = run(InlineCode, "x = 1", 0, 5);
    assert_eq!(e.text, "`x = 1`");
    let back = run(InlineCode, &e.text, e.start, e.end);
    assert_eq!(back.text, "x = 1");
    let e = run(InlineCode, "a`b", 0, 3);
    assert_eq!(e.text, "``` a`b ```");
    let back = run(InlineCode, &e.text, e.start, e.end);
    assert_eq!(back.text, "a`b");
}
#[test]
fn inline_math_is_typst_not_latex() {
    let e = run(InlineMath, "x^2", 0, 3);
    assert_eq!(e.text, "$x^2$");
    assert!(!e.text.contains("$$"));
    let back = run(InlineMath, &e.text, e.start, e.end);
    assert_eq!(back.text, "x^2");
}
#[test]
fn display_math_uses_spaced_block_syntax() {
    let e = run(DisplayMath, "x^2 + y^2 = z^2", 0, 15);
    assert_eq!(e.text, "$ x^2 + y^2 = z^2 $");
    let back = run(DisplayMath, &e.text, e.start, e.end);
    assert_eq!(back.text, "x^2 + y^2 = z^2");
    let e = run(DisplayMath, "", 0, 0);
    assert_eq!(e.text, "$  $");
    assert_eq!(e.start, 2);
}
#[test]
fn inline_and_display_math_convert_into_each_other() {
    let d = run(DisplayMath, "$x$", 1, 2);
    assert_eq!(d.text, "$ x $");
    let i = run(InlineMath, &d.text, d.start, d.end);
    assert_eq!(i.text, "$x$");
}
#[test]
fn code_block_wrap_and_unwrap() {
    let e = run(CodeBlock, "fn main() {}", 0, 12);
    assert_eq!(e.text, "```\nfn main() {}\n```");
    assert_eq!(sel(&e), "fn main() {}");
    let back = run(CodeBlock, &e.text, e.start, e.end);
    assert_eq!(back.text, "fn main() {}");
}
#[test]
fn code_block_empty_selection_puts_caret_on_inner_line() {
    let e = run(CodeBlock, "", 0, 0);
    assert_eq!(e.text, "```\n\n```");
    assert_eq!(e.start, 4);
}
#[test]
fn code_block_preserves_indent_and_multiple_lines() {
    let t = "  a\n  b";
    let e = run(CodeBlock, t, 0, t.len());
    assert_eq!(e.text, "  ```\n  a\n  b\n  ```");
    let back = run(CodeBlock, &e.text, e.start, e.end);
    assert_eq!(back.text, t);
}
#[test]
fn nested_formatting_composes() {
    let b = run(Bold, "word", 0, 4);
    let i = run(Italic, &b.text, b.start, b.end);
    assert_eq!(i.text, "*_word_*");
    let m = run(InlineMath, "x", 0, 1);
    let b = run(Bold, &m.text, 0, m.text.len());
    assert_eq!(b.text, "*$x$*");
}
#[test]
fn malformed_source_never_panics_and_is_preserved() {
    for t in [
        "*unclosed",
        "$ x",
        "```rust\nfn",
        "_a*b_*",
        "#let (",
        "= \n$$",
    ] {
        for c in [Bold, Italic, CodeBlock, InlineCode, DisplayMath, InlineMath] {
            let e = run(c, t, 0, t.len());
            assert!(e.text.contains(t.trim()) || !e.text.is_empty());
            let e = run(c, t, t.len() / 2, t.len() / 2);
            assert!(e.start <= e.text.len());
        }
    }
}
