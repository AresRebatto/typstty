//! Pure editing commands (the six shortcuts). They operate on a whole document
//! string with *byte* offsets and return the new text plus the new selection.
//! The Typst syntax tree decides whether a selection is already formatted, so
//! `*a* b *c*` is never mistaken for one bold span.

use std::ops::Range;
use typst_syntax::{LinkedNode, SyntaxKind, parse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Bold,
    Italic,
    CodeBlock,
    InlineCode,
    DisplayMath,
    InlineMath,
}

/// State kept between commands so that a toggle off / on round-trips (E3):
/// the language tag of the last unwrapped fenced block and its body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandContext {
    pub code_lang: String,
    pub code_body: String,
}

pub fn apply(cmd: Command, text: &str, start: usize, end: usize) -> Edit {
    apply_ctx(cmd, text, start, end, &mut CommandContext::default())
}

pub fn apply_ctx(
    cmd: Command,
    text: &str,
    start: usize,
    end: usize,
    ctx: &mut CommandContext,
) -> Edit {
    let (s, e) = clamp(text, start.min(end), start.max(end));
    match cmd {
        Command::Bold => toggle_symmetric(text, s, e, SyntaxKind::Strong, "*"),
        Command::Italic => toggle_symmetric(text, s, e, SyntaxKind::Emph, "_"),
        Command::InlineCode => toggle_inline_code(text, s, e),
        Command::CodeBlock => toggle_code_block(text, s, e, ctx),
        Command::InlineMath => toggle_math(text, s, e, false),
        Command::DisplayMath => toggle_math(text, s, e, true),
    }
}

fn clamp(text: &str, mut s: usize, mut e: usize) -> (usize, usize) {
    s = s.min(text.len());
    e = e.min(text.len());
    while !text.is_char_boundary(s) {
        s -= 1;
    }
    while !text.is_char_boundary(e) {
        e += 1;
    }
    (s, e)
}

/// Innermost node of `kind` that fully contains `s..e`.
fn find_node(text: &str, kind: SyntaxKind, s: usize, e: usize) -> Option<Range<usize>> {
    let root = parse(text);
    let linked = LinkedNode::new(&root);
    let mut found = None;
    fn walk(n: &LinkedNode, kind: SyntaxKind, s: usize, e: usize, out: &mut Option<Range<usize>>) {
        let r = n.range();
        if r.start > s || r.end < e {
            return;
        }
        if n.kind() == kind {
            *out = Some(r);
        }
        for c in n.children() {
            walk(&c, kind, s, e, out);
        }
    }
    walk(&linked, kind, s, e, &mut found);
    found
}

fn is_ws(c: char) -> bool {
    c.is_whitespace()
}

/// Non-blank paragraph groups inside `s..e`, trimmed of surrounding whitespace.
fn segments(text: &str, s: usize, e: usize) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut cur: Option<Range<usize>> = None;
    let mut pos = s;
    for line in text[s..e].split_inclusive('\n') {
        let lo = pos;
        pos += line.len();
        let t = line.trim_matches(is_ws);
        if t.is_empty() {
            if let Some(c) = cur.take() {
                out.push(c);
            }
            continue;
        }
        let a = lo + (line.len() - line.trim_start_matches(is_ws).len());
        let b = a + t.len();
        match &mut cur {
            Some(c) => c.end = b,
            None => cur = Some(a..b),
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

fn wrap_trimmed(piece: &str, d: &str) -> String {
    // keep leading/trailing whitespace outside the delimiters
    let t = piece.trim_matches(is_ws);
    if t.is_empty() {
        return piece.to_string();
    }
    let lead = &piece[..piece.len() - piece.trim_start_matches(is_ws).len()];
    let trail = &piece[lead.len() + t.len()..];
    format!("{lead}{d}{t}{d}{trail}")
}

/// Length in bytes of a block-level marker at the start of `content` (after the
/// indentation): `= `, `== `, `- `, `+ `, `1. `, `/ term: ` (E1).
fn marker_len(content: &str) -> usize {
    let eq = content.chars().take_while(|&c| c == '=').count();
    if eq > 0 && content[eq..].starts_with(' ') {
        return eq + 1;
    }
    if content.starts_with("- ") || content.starts_with("+ ") {
        return 2;
    }
    let digits = content.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && content[digits..].starts_with(". ") {
        return digits + 2;
    }
    if let Some(rest) = content.strip_prefix("/ ") {
        return match rest.find(": ") {
            Some(i) => 2 + i + 2,
            None => 2,
        };
    }
    0
}

/// Content ranges (after the marker) of the lines of `s..e`, when the selection
/// should be treated line by line: several lines with at least one marker, or a
/// single marker line whose selection starts at/before the marker (E1).
/// Delimiters `d` sitting just outside a content range are included, so that a
/// second press toggles the formatting off again.
fn marker_lines(text: &str, s: usize, e: usize, d: &str) -> Option<Vec<Range<usize>>> {
    let dl = d.len();
    let mut out = Vec::new();
    let mut any_marker = false;
    let mut covers_marker = false;
    let mut nonblank = 0;
    let ls = line_start(text, s);
    let mut pos = ls;
    for line in text[ls..].split_inclusive('\n') {
        let lo = pos;
        pos += line.len();
        if lo > e || (lo == e && e > s) {
            break;
        }
        let body = line.trim_end_matches(['\n', '\r']);
        let content = body.trim_start();
        if content.is_empty() {
            continue;
        }
        nonblank += 1;
        let ind = body.len() - content.len();
        let m = marker_len(content);
        if m > 0 {
            any_marker = true;
            covers_marker |= s <= lo + ind;
        }
        let mut a = lo + ind + m;
        let mut b = lo + body.trim_end().len();
        if a >= b {
            continue;
        }
        // delimiters just outside the content range belong to it
        if !text[a..b].starts_with(d) && a >= lo + ind + m + dl && text[..a].ends_with(d) {
            a -= dl;
        }
        if !text[a..b].ends_with(d) && b + dl <= lo + body.len() && text[b..].starts_with(d) {
            b += dl;
        }
        out.push(a..b);
    }
    let trigger = any_marker && (nonblank >= 2 || covers_marker);
    if trigger && !out.is_empty() {
        Some(out)
    } else {
        None
    }
}

fn is_wrapped(t: &str, d: &str) -> bool {
    let dl = d.len();
    t.len() >= 2 * dl && t.starts_with(d) && t.ends_with(d) && !t[dl..t.len() - dl].contains(d)
}

/// Wrap/unwrap the *content* of every line, keeping `= `, `- `, `+ `, `/ ` markers (E1).
fn toggle_lines(text: &str, lines: &[Range<usize>], d: &str) -> Edit {
    let dl = d.len();
    let all = lines
        .iter()
        .all(|r| is_wrapped(text[r.clone()].trim_matches(is_ws), d));
    let mut out = String::from(&text[..lines[0].start]);
    let (mut ss, mut se) = (0, 0);
    let mut pos = lines[0].start;
    for (i, r) in lines.iter().enumerate() {
        out.push_str(&text[pos..r.start]);
        let seg = &text[r.clone()];
        let t = seg.trim_matches(is_ws);
        let lead = &seg[..seg.len() - seg.trim_start_matches(is_ws).len()];
        let trail = &seg[lead.len() + t.len()..];
        out.push_str(lead);
        let begin;
        if all {
            begin = out.len();
            out.push_str(&t[dl..t.len() - dl]);
        } else if is_wrapped(t, d) {
            begin = out.len() + dl;
            out.push_str(t);
        } else {
            out.push_str(d);
            begin = out.len();
            out.push_str(t);
            out.push_str(d);
        }
        let inner = if all || is_wrapped(t, d) {
            t.len() - 2 * dl
        } else {
            t.len()
        };
        out.push_str(trail);
        if i == 0 {
            ss = begin;
        }
        se = begin + inner;
        pos = r.end;
    }
    out.push_str(&text[pos..]);
    Edit {
        text: out,
        start: ss,
        end: se,
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

/// `*`/`_` do not format in the middle of a word in Typst, so for an
/// intra-word selection the functional form `#strong[..]` / `#emph[..]` is used (E2).
fn intra_word(text: &str, s: usize, e: usize) -> bool {
    let before = text[..s].chars().next_back();
    let after = text[e..].chars().next();
    let first = text[s..e].chars().next();
    let last = text[s..e].chars().next_back();
    before
        .zip(first)
        .is_some_and(|(b, f)| is_word(b) && is_word(f))
        || last
            .zip(after)
            .is_some_and(|(l, a)| is_word(l) && is_word(a))
}

fn functional_name(kind: SyntaxKind) -> &'static str {
    if kind == SyntaxKind::Strong {
        "strong"
    } else {
        "emph"
    }
}

fn toggle_functional(text: &str, s: usize, e: usize, name: &str) -> Edit {
    let open = format!("#{name}[");
    // already wrapped just outside the selection → unwrap
    if text[..s].ends_with(&open) && text[e..].starts_with(']') {
        let new = format!(
            "{}{}{}",
            &text[..s - open.len()],
            &text[s..e],
            &text[e + 1..]
        );
        let st = s - open.len();
        return Edit {
            text: new,
            start: st,
            end: st + (e - s),
        };
    }
    let new = format!("{}{open}{}]{}", &text[..s], &text[s..e], &text[e..]);
    Edit {
        text: new,
        start: s + open.len(),
        end: s + open.len() + (e - s),
    }
}

fn toggle_symmetric(text: &str, s: usize, e: usize, kind: SyntaxKind, d: &str) -> Edit {
    let dl = d.len();
    let segs = segments(text, s, e);

    // Empty (or whitespace-only) selection.
    if segs.is_empty() {
        if let Some(r) = find_node(text, kind, s, e)
            && r.len() >= 2 * dl
            && text[r.clone()].starts_with(d)
            && text[r.clone()].ends_with(d)
        {
            let inner = &text[r.start + dl..r.end - dl];
            let new = format!("{}{}{}", &text[..r.start], inner, &text[r.end..]);
            let caret = (s - dl).max(r.start).min(r.start + inner.len());
            return Edit {
                text: new,
                start: caret,
                end: caret,
            };
        }
        let mut new = String::with_capacity(text.len() + 2 * dl);
        new.push_str(&text[..s]);
        new.push_str(d);
        new.push_str(d);
        new.push_str(&text[e..]);
        return Edit {
            text: new,
            start: s + dl,
            end: s + dl,
        };
    }

    // Already formatted? (single segment only; syntax tree decides)
    if segs.len() == 1 {
        let seg = segs[0].clone();
        if let Some(r) = find_node(text, kind, seg.start, seg.end) {
            let node = &text[r.clone()];
            if node.len() >= 2 * dl && node.starts_with(d) && node.ends_with(d) {
                let inner = r.start + dl..r.end - dl;
                let covers = seg.start <= r.start + dl && seg.end >= r.end - dl;
                if covers {
                    // unwrap the whole span
                    let new = format!(
                        "{}{}{}",
                        &text[..r.start],
                        &text[inner.clone()],
                        &text[r.end..]
                    );
                    return Edit {
                        text: new,
                        start: r.start,
                        end: r.start + inner.len(),
                    };
                }
                // selection strictly inside: split the span around it
                let left = &text[inner.start..seg.start];
                let mid = &text[seg.start..seg.end];
                let right = &text[seg.end..inner.end];
                let mut new = String::from(&text[..r.start]);
                new.push_str(&wrap_trimmed(left, d));
                let ms = new.len();
                new.push_str(mid);
                let me = new.len();
                new.push_str(&wrap_trimmed(right, d));
                new.push_str(&text[r.end..]);
                return Edit {
                    text: new,
                    start: ms,
                    end: me,
                };
            }
        }
    }

    // Block markers (`= `, `- `, `+ `, `/ `): wrap the content of each line (E1).
    if !text[s..e].is_empty()
        && let Some(lines) = marker_lines(text, s, e, d)
    {
        return toggle_lines(text, &lines, d);
    }
    // Intra-word selection on a single line: use #strong[..] / #emph[..] (E2).
    if segs.len() == 1 && !text[s..e].contains('\n') {
        let (a, b) = (segs[0].start, segs[0].end);
        let name = functional_name(kind);
        let wrapped_fn = text[..a].ends_with(&format!("#{name}[")) && text[b..].starts_with(']');
        if wrapped_fn || intra_word(text, a, b) {
            return toggle_functional(text, a, b, name);
        }
    }
    // Multi-paragraph selections (e.g. the selection left by a previous wrap
    // excludes the delimiters): treat delimiters just outside a segment as part of it.
    let segs: Vec<Range<usize>> = if segs.len() > 1 {
        segs.into_iter()
            .map(|r| {
                let seg = &text[r.clone()];
                let before = r.start >= dl && text[..r.start].ends_with(d);
                let after = r.end + dl <= text.len() && text[r.end..].starts_with(d);
                let (sd, ed) = (seg.starts_with(d), seg.ends_with(d));
                if (sd || before)
                    && (ed || after)
                    && (sd || before || ed || after)
                    && (before || after)
                {
                    let a = if !sd && before { r.start - dl } else { r.start };
                    let b = if !ed && after { r.end + dl } else { r.end };
                    a..b
                } else {
                    r
                }
            })
            .collect()
    } else {
        segs
    };
    // Wrap each paragraph segment that is not already wrapped.
    let wrapped = |a: usize, b: usize| {
        let t = &text[a..b];
        t.len() >= 2 * dl && t.starts_with(d) && t.ends_with(d) && !t[dl..t.len() - dl].contains(d)
    };
    let all_wrapped = segs.iter().all(|r| wrapped(r.start, r.end));
    let mut out = String::from(&text[..segs[0].start]);
    let mut sel_start = 0;
    let mut sel_end = 0;
    let mut pos = segs[0].start;
    for (i, r) in segs.iter().enumerate() {
        out.push_str(&text[pos..r.start]);
        let seg = &text[r.clone()];
        let (inner_len, begin);
        if all_wrapped {
            begin = out.len();
            out.push_str(&seg[dl..seg.len() - dl]);
            inner_len = seg.len() - 2 * dl;
        } else if wrapped(r.start, r.end) {
            begin = out.len() + dl;
            out.push_str(seg);
            inner_len = seg.len() - 2 * dl;
            // selection excludes the delimiters in both cases
            let _ = inner_len;
            if i == 0 {
                sel_start = begin;
            }
            sel_end = begin + seg.len() - 2 * dl;
            pos = r.end;
            continue;
        } else {
            out.push_str(d);
            begin = out.len();
            out.push_str(seg);
            out.push_str(d);
            inner_len = seg.len();
        }
        if i == 0 {
            sel_start = begin;
        }
        sel_end = begin + inner_len;
        pos = r.end;
    }
    out.push_str(&text[pos..]);
    Edit {
        text: out,
        start: sel_start,
        end: sel_end,
    }
}

fn max_tick_run(s: &str) -> usize {
    let (mut best, mut cur) = (0, 0);
    for c in s.chars() {
        if c == '`' {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

fn toggle_inline_code(text: &str, s: usize, e: usize) -> Edit {
    // existing raw span containing the selection (inline form only)
    if let Some(r) = find_node(text, SyntaxKind::Raw, s, e) {
        let node = &text[r.clone()];
        let k = node.chars().take_while(|&c| c == '`').count();
        let is_block = k >= 3 && node.contains('\n');
        if !is_block && k >= 1 && node.len() > 2 * k && node.ends_with(&"`".repeat(k)) {
            let mut inner = &node[k..node.len() - k];
            if k >= 3 {
                inner = inner.strip_prefix(' ').unwrap_or(inner);
                inner = inner.strip_suffix(' ').unwrap_or(inner);
            }
            let new = format!("{}{}{}", &text[..r.start], inner, &text[r.end..]);
            return Edit {
                text: new,
                start: r.start,
                end: r.start + inner.len(),
            };
        }
        if !is_block && k >= 1 && node.len() == 2 * k {
            // empty raw: remove
            let new = format!("{}{}", &text[..r.start], &text[r.end..]);
            return Edit {
                text: new,
                start: r.start,
                end: r.start,
            };
        }
    }
    let sel = &text[s..e];
    let run = max_tick_run(sel);
    let n = if run == 0 { 1 } else { (run + 1).max(3) };
    let ticks = "`".repeat(n);
    let pad = if n >= 3 { " " } else { "" };
    if sel.is_empty() {
        let new = format!("{}{ticks}{pad}{pad}{ticks}{}", &text[..s], &text[e..]);
        let c = s + n + pad.len();
        return Edit {
            text: new,
            start: c,
            end: c,
        };
    }
    let new = format!("{}{ticks}{pad}{sel}{pad}{ticks}{}", &text[..s], &text[e..]);
    let st = s + n + pad.len();
    Edit {
        text: new,
        start: st,
        end: st + sel.len(),
    }
}

fn line_start(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0)
}
fn line_end(text: &str, pos: usize) -> usize {
    text[pos..]
        .find('\n')
        .map(|i| pos + i)
        .unwrap_or(text.len())
}
fn indent_of(text: &str, ls: usize) -> String {
    text[ls..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

fn toggle_code_block(text: &str, s: usize, e: usize, ctx: &mut CommandContext) -> Edit {
    // Existing fenced raw block around / equal to the selection?
    if let Some(r) = find_node(text, SyntaxKind::Raw, s, e) {
        let node = &text[r.clone()];
        let k = node.chars().take_while(|&c| c == '`').count();
        if k >= 3 && node.contains('\n') && node.ends_with(&"`".repeat(k)) {
            let first_nl = node.find('\n').unwrap();
            let last_nl = node.rfind('\n').unwrap();
            if last_nl >= first_nl {
                let body = if last_nl > first_nl {
                    &node[first_nl + 1..last_nl]
                } else {
                    ""
                };
                // remember the language tag so that re-wrapping restores it (E3)
                let lang = node[k..first_nl].trim();
                if !lang.is_empty() {
                    ctx.code_lang = lang.to_string();
                    ctx.code_body = body.to_string();
                }
                let ls = line_start(text, r.start);
                let from = if text[ls..r.start].trim().is_empty() {
                    ls
                } else {
                    r.start
                };
                let new = format!("{}{}{}", &text[..from], body, &text[r.end..]);
                return Edit {
                    text: new,
                    start: from,
                    end: from + body.len(),
                };
            }
        }
    }
    let ls = line_start(text, s);
    // do not swallow the following line when the selection ends right after a newline
    let e_eff = if e > s && text[..e].ends_with('\n') {
        e - 1
    } else {
        e
    };
    let le = line_end(text, e_eff);
    let ind = indent_of(text, ls);
    let body = &text[ls..le];
    let body_empty = body.trim().is_empty();
    let fence = {
        let run = max_tick_run(body);
        "`".repeat((run + 1).max(3))
    };
    if body_empty {
        let new = format!(
            "{}{ind}{fence}\n{ind}\n{ind}{fence}{}",
            &text[..ls],
            &text[le..]
        );
        let c = ls + ind.len() + fence.len() + 1 + ind.len();
        return Edit {
            text: new,
            start: c,
            end: c,
        };
    }
    // restore the language of the block that was just unwrapped (E3)
    let lang = if !ctx.code_lang.is_empty() && ctx.code_body == body {
        ctx.code_lang.clone()
    } else {
        String::new()
    };
    let mut new = String::from(&text[..ls]);
    new.push_str(&ind);
    new.push_str(&fence);
    new.push_str(&lang);
    new.push('\n');
    let st = new.len();
    new.push_str(body);
    let en = new.len();
    new.push('\n');
    new.push_str(&ind);
    new.push_str(&fence);
    new.push_str(&text[le..]);
    Edit {
        text: new,
        start: st,
        end: en,
    }
}

fn toggle_math(text: &str, s: usize, e: usize, display: bool) -> Edit {
    if let Some(r) = find_node(text, SyntaxKind::Equation, s, e) {
        let node = &text[r.clone()];
        if node.len() >= 2 && node.starts_with('$') && node.ends_with('$') {
            let inner = &node[1..node.len() - 1];
            let is_display = inner.starts_with(is_ws) && inner.ends_with(is_ws);
            let core = inner.trim_matches(is_ws);
            let (replacement, cs, ce);
            if is_display == display {
                // same kind: unwrap
                replacement = core.to_string();
                cs = r.start;
                ce = r.start + core.len();
            } else if display {
                // inline -> display
                if core.contains('\n') {
                    let ind = indent_of(text, line_start(text, r.start));
                    let body = reindent(core, &format!("{ind}  "));
                    replacement = format!("$\n{body}\n{ind}$");
                    cs = r.start + 2;
                    let new = format!("{}{}{}", &text[..r.start], replacement, &text[r.end..]);
                    return Edit {
                        text: new,
                        start: cs,
                        end: cs + body.len(),
                    };
                } else {
                    replacement = format!("$ {core} $");
                    cs = r.start + 2;
                }
                ce = cs + core.len();
            } else {
                // display -> inline
                replacement = format!("${core}$");
                cs = r.start + 1;
                ce = cs + core.len();
            }
            let new = format!("{}{}{}", &text[..r.start], replacement, &text[r.end..]);
            return Edit {
                text: new,
                start: cs,
                end: ce,
            };
        }
    }
    let sel = &text[s..e];
    if !display {
        let new = format!("{}${sel}${}", &text[..s], &text[e..]);
        return Edit {
            text: new,
            start: s + 1,
            end: s + 1 + sel.len(),
        };
    }
    if sel.contains('\n') {
        let ls = line_start(text, s);
        let ind = indent_of(text, ls);
        let body = reindent(sel, &format!("{ind}  "));
        let new = format!("{}$\n{body}\n{ind}${}", &text[..s], &text[e..]);
        return Edit {
            text: new,
            start: s + 2,
            end: s + 2 + body.len(),
        };
    }
    let new = format!("{}$ {sel} ${}", &text[..s], &text[e..]);
    Edit {
        text: new,
        start: s + 2,
        end: s + 2 + sel.len(),
    }
}

/// Dedent `body` by its common indentation, then indent every non-blank line
/// with `ind` (E4).
fn reindent(body: &str, ind: &str) -> String {
    let common = body
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let lines: Vec<&str> = body.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        let stripped = if i == 0 {
            l.trim_start()
        } else {
            &l[common.min(l.len() - l.trim_start().len())..]
        };
        if stripped.trim().is_empty() {
            out.push(String::new());
        } else {
            out.push(format!("{ind}{stripped}"));
        }
    }
    out.join("\n")
}
