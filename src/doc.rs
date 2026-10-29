//! Document model: Typst source is the single source of truth. The visual
//! layer works on *blocks* (byte ranges into the source); rendering never
//! rewrites the text, and edits only replace the range of the active block.

use std::ops::Range;
use std::time::{Duration, Instant};
use typst_syntax::{SyntaxKind, parse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Heading,
    Definition, // #set / #show / #let / #import
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub range: Range<usize>,
    pub kind: BlockKind,
}

fn own_line(text: &str, r: &Range<usize>) -> bool {
    let before = &text[..r.start];
    let line_before = before.rsplit('\n').next().unwrap_or("");
    let after = &text[r.end..];
    let line_after = after.split('\n').next().unwrap_or("");
    line_before.trim().is_empty() && line_after.trim().is_empty()
}

/// Split the source into top-level blocks using the real Typst parser.
/// Blocks are whitespace-trimmed ranges; everything between them is
/// whitespace, so the source is always fully covered.
pub fn segment(text: &str) -> Vec<Block> {
    let root = parse(text);
    let kids: Vec<_> = root.children().collect();
    let mut offs = Vec::with_capacity(kids.len());
    let mut p = 0;
    for k in &kids {
        offs.push(p);
        p += k.len();
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut cur: Option<Range<usize>> = None;
    macro_rules! flush {
        () => {
            if let Some(r) = cur.take() {
                blocks.push(Block {
                    range: r,
                    kind: BlockKind::Content,
                });
            }
        };
    }
    let mut i = 0;
    while i < kids.len() {
        let k = &kids[i];
        let r = offs[i]..offs[i] + k.len();
        match k.kind() {
            SyntaxKind::Space => {}
            SyntaxKind::Parbreak => flush!(),
            SyntaxKind::Heading => {
                flush!();
                blocks.push(Block {
                    range: r,
                    kind: BlockKind::Heading,
                });
            }
            SyntaxKind::Hash
                if i + 1 < kids.len()
                    && matches!(
                        kids[i + 1].kind(),
                        SyntaxKind::SetRule
                            | SyntaxKind::ShowRule
                            | SyntaxKind::LetBinding
                            | SyntaxKind::ModuleImport
                    ) =>
            {
                let rr = r.start..offs[i + 1] + kids[i + 1].len();
                if own_line(text, &rr) {
                    flush!();
                    blocks.push(Block {
                        range: rr,
                        kind: BlockKind::Definition,
                    });
                    i += 2;
                    continue;
                }
                match &mut cur {
                    Some(c) => c.end = rr.end,
                    None => cur = Some(rr.clone()),
                }
                i += 2;
                continue;
            }
            SyntaxKind::Equation | SyntaxKind::Raw
                if own_line(text, &r)
                    && (k.kind() == SyntaxKind::Equation || text[r.clone()].contains('\n')) =>
            {
                flush!();
                blocks.push(Block {
                    range: r,
                    kind: BlockKind::Content,
                });
            }
            _ => match &mut cur {
                Some(c) => c.end = r.end,
                None => cur = Some(r),
            },
        }
        i += 1;
    }
    flush!();
    blocks
}

/// The range the caret edits: a real block, a block extended over at most one
/// newline (so Enter inside a list/paragraph does not leave the block), or an
/// empty virtual range at the caret.
pub fn active_range(text: &str, blocks: &[Block], caret: usize) -> Range<usize> {
    for b in blocks {
        if b.range.start <= caret && caret <= b.range.end {
            return b.range.clone();
        }
    }
    let ws_only = |s: &str| s.chars().all(char::is_whitespace) && s.matches('\n').count() <= 1;
    for b in blocks {
        if b.range.end < caret && ws_only(&text[b.range.end..caret]) {
            return b.range.start..caret;
        }
    }
    for b in blocks {
        if caret < b.range.start && ws_only(&text[caret..b.range.start]) {
            return caret..b.range.end;
        }
    }
    caret..caret
}

/// Line-ending style detected when a file is loaded (E7). The in-memory text is
/// always LF-normalised; the original style is restored on save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

/// Strip a UTF-8 BOM and normalise CRLF to LF. Returns (text, had_bom, ending).
pub fn normalize_input(raw: &str) -> (String, bool, LineEnding) {
    let (body, bom) = match raw.strip_prefix('\u{feff}') {
        Some(b) => (b, true),
        None => (raw, false),
    };
    if body.contains("\r\n") {
        (body.replace("\r\n", "\n"), bom, LineEnding::CrLf)
    } else {
        (body.to_string(), bom, LineEnding::Lf)
    }
}

/// Inverse of [`normalize_input`] used when saving.
pub fn denormalize_output(text: &str, bom: bool, ending: LineEnding) -> String {
    let mut out = String::with_capacity(text.len() + 3);
    if bom {
        out.push('\u{feff}');
    }
    match ending {
        LineEnding::Lf => out.push_str(text),
        LineEnding::CrLf => out.push_str(&text.replace('\n', "\r\n")),
    }
    out
}

/// Atomic save (F1): write to a temporary sibling, fsync, then rename over the
/// target, so a crash or a full disk never leaves a truncated file.
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    DocStart,
    DocEnd,
}

/// Where the head goes for a navigation key while the selection is managed at
/// document level (shift-extend across blocks, E10/D3).
pub fn move_head(text: &str, blocks: &[Block], head: usize, dir: Dir) -> usize {
    let head = head.min(text.len());
    match dir {
        Dir::Left => text[..head]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0),
        Dir::Right => text[head..]
            .chars()
            .next()
            .map(|c| head + c.len_utf8())
            .unwrap_or(text.len()),
        Dir::Up => blocks
            .iter()
            .rev()
            .find(|b| b.range.start < head)
            .map(|b| b.range.start)
            .unwrap_or(0),
        Dir::Down => blocks
            .iter()
            .find(|b| b.range.end > head)
            .map(|b| b.range.end)
            .unwrap_or(text.len()),
        Dir::Home => text[..head].rfind('\n').map(|i| i + 1).unwrap_or(0),
        Dir::End => text[head..]
            .find('\n')
            .map(|i| head + i)
            .unwrap_or(text.len()),
        Dir::DocStart => 0,
        Dir::DocEnd => text.len(),
    }
}

/// True when `a` and `b` lie inside one block (so a plain text editor can own the selection).
pub fn within_one_block(blocks: &[Block], a: usize, b: usize) -> bool {
    let (lo, hi) = (a.min(b), a.max(b));
    blocks
        .iter()
        .any(|bl| bl.range.start <= lo && hi <= bl.range.end)
}

/// Result of a block-merging deletion across a block boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub text: String,
    pub caret: usize,
}

/// Backspace at the very start of a block (U1/E9): remove the whole whitespace
/// gap between the previous block and this one, join them with the smallest
/// separator that keeps them in one paragraph, and put the caret at the end of
/// the previous block. Headings and definitions are never fused with a
/// neighbour: there the gap is just collapsed to a single newline.
pub fn backspace_at_block_start(
    text: &str,
    blocks: &[Block],
    caret: usize,
) -> Option<MergeOutcome> {
    let idx = blocks.iter().position(|b| b.range.start == caret)?;
    if idx == 0 {
        // first block: only leading whitespace can be removed
        if caret == 0 {
            return None;
        }
        let mut t = text.to_string();
        t.replace_range(0..caret, "");
        return Some(MergeOutcome { text: t, caret: 0 });
    }
    let prev = &blocks[idx - 1];
    let cur = &blocks[idx];
    join_blocks(text, prev, cur)
}

/// Delete at the very end of a block: symmetric to [`backspace_at_block_start`].
pub fn delete_at_block_end(text: &str, blocks: &[Block], caret: usize) -> Option<MergeOutcome> {
    let idx = blocks.iter().position(|b| b.range.end == caret)?;
    if idx + 1 >= blocks.len() {
        if caret >= text.len() {
            return None;
        }
        let mut t = text.to_string();
        t.replace_range(caret.., "");
        return Some(MergeOutcome { text: t, caret });
    }
    join_blocks(text, &blocks[idx], &blocks[idx + 1])
}

fn join_blocks(text: &str, prev: &Block, cur: &Block) -> Option<MergeOutcome> {
    let gap = prev.range.end..cur.range.start;
    if gap.start > gap.end {
        return None;
    }
    let structural = |k: BlockKind| matches!(k, BlockKind::Heading | BlockKind::Definition);
    let sep = if structural(prev.kind) || structural(cur.kind) {
        "\n"
    } else {
        " "
    };
    // A one-char gap already is the minimal separator: still collapse it so the
    // caret lands at the end of the previous block.
    let mut t = String::with_capacity(text.len());
    t.push_str(&text[..gap.start]);
    t.push_str(sep);
    t.push_str(&text[gap.end..]);
    Some(MergeOutcome {
        text: t,
        caret: prev.range.end,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
}

type BlockCache = (u64, usize, std::rc::Rc<Vec<Block>>);

/// Source text + selection + undo history. No UI types in here.
#[derive(Debug)]
pub struct Document {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
    saved: String,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    cur: Snapshot,
    last_edit: Option<Instant>,
    pub coalesce: Duration,
    pub version: u64,
    /// Line-ending style / BOM of the file on disk (E7).
    pub line_ending: LineEnding,
    pub had_bom: bool,
    /// `version` at the last save, so `is_dirty` is O(1) (P1).
    saved_version: u64,
    block_cache: std::cell::RefCell<Option<BlockCache>>,
    /// Selection spans the whole document (second Ctrl+A, U4).
    pub multi: bool,
}

impl Document {
    pub fn new(text: String) -> Self {
        let cur = Snapshot {
            text: text.clone(),
            anchor: 0,
            head: 0,
        };
        Self {
            saved: text.clone(),
            text,
            anchor: 0,
            head: 0,
            undo: vec![],
            redo: vec![],
            cur,
            last_edit: None,
            coalesce: Duration::from_millis(800),
            version: 0,
            line_ending: LineEnding::Lf,
            had_bom: false,
            saved_version: 0,
            block_cache: std::cell::RefCell::new(None),
            multi: false,
        }
    }
    /// O(1) in the common case: equal versions mean equal text; only when the
    /// version moved do we compare (so undoing back to the saved text is clean).
    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version && self.text != self.saved
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.text.clone();
        self.saved_version = self.version;
    }
    /// Bytes to write to disk: original BOM / line endings restored (E7).
    pub fn disk_bytes(&self) -> Vec<u8> {
        denormalize_output(&self.text, self.had_bom, self.line_ending).into_bytes()
    }
    /// Load raw file contents, remembering BOM and line endings (E7).
    pub fn load_raw(&mut self, raw: &str) {
        let (t, bom, le) = normalize_input(raw);
        self.load(t);
        self.had_bom = bom;
        self.line_ending = le;
    }
    /// Ctrl+A: first press selects the current block, second the whole document (U4).
    pub fn select_all_step(&mut self, active: Range<usize>) {
        let already_block = self.sel() == active && !active.is_empty();
        if self.multi || already_block || active.is_empty() {
            self.anchor = 0;
            self.head = self.text.len();
            self.multi = true;
        } else {
            self.anchor = active.start;
            self.head = active.end;
        }
    }
    /// Any caret move / click / arrow key drops the whole-document mode.
    pub fn collapse_multi(&mut self) {
        self.multi = false;
    }
    /// Replace the current selection (possibly spanning blocks) with `s`.
    pub fn replace_selection(&mut self, s: &str) {
        let r = self.sel();
        self.text.replace_range(r.clone(), s);
        let c = r.start + s.len();
        self.anchor = c;
        self.head = c;
        self.multi = false;
        self.commit(true);
    }
    /// Apply a block-merge outcome as a single undo step.
    pub fn apply_merge(&mut self, m: MergeOutcome) {
        self.text = m.text;
        self.anchor = m.caret;
        self.head = m.caret;
        self.multi = false;
        self.commit(true);
    }
    pub fn sel(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = anchor.min(self.text.len());
        self.head = head.min(self.text.len());
        self.multi = false;
    }
    /// Selection managed at document level, possibly spanning blocks (U4/E10/D3).
    pub fn set_multi_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = anchor.min(self.text.len());
        self.head = head.min(self.text.len());
        self.multi = true;
    }
    /// Segmentation is cached per document version and text length (P1): the
    /// UI calls this every frame, but the parser only runs after an edit.
    pub fn blocks(&self) -> Vec<Block> {
        self.blocks_rc().as_ref().clone()
    }
    pub fn blocks_rc(&self) -> std::rc::Rc<Vec<Block>> {
        if let Some((v, l, b)) = self.block_cache.borrow().as_ref()
            && *v == self.version
            && *l == self.text.len()
        {
            return b.clone();
        }
        let b = std::rc::Rc::new(segment(&self.text));
        *self.block_cache.borrow_mut() = Some((self.version, self.text.len(), b.clone()));
        b
    }
    /// Record the current text/selection as a history state.
    /// `boundary` forces a new undo step (commands, paste); otherwise typing
    /// bursts are coalesced.
    pub fn commit(&mut self, boundary: bool) {
        if self.text == self.cur.text {
            // selection only: no allocation (P1)
            self.cur.anchor = self.anchor;
            self.cur.head = self.head;
            return;
        }
        let snap = Snapshot {
            text: self.text.clone(),
            anchor: self.anchor,
            head: self.head,
        };
        self.version += 1;
        let now = Instant::now();
        let coalesce = !boundary
            && self
                .last_edit
                .is_some_and(|t| now.duration_since(t) < self.coalesce)
            && !self.undo.is_empty();
        if coalesce {
            self.cur = snap;
        } else {
            let prev = std::mem::replace(&mut self.cur, snap);
            self.undo.push(prev);
            self.redo.clear();
        }
        self.last_edit = Some(now);
    }
    /// Replace `range` with `new` (used by the active block editor).
    pub fn replace_range(&mut self, range: Range<usize>, new: &str, boundary: bool) {
        self.text.replace_range(range, new);
        self.commit(boundary);
    }
    pub fn apply_edit(&mut self, e: crate::commands::Edit) {
        self.text = e.text;
        self.anchor = e.start;
        self.head = e.end;
        self.multi = false;
        self.commit(true);
    }
    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.cur, prev.clone()));
            self.restore(&prev);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.cur, next.clone()));
            self.restore(&next);
            true
        } else {
            false
        }
    }
    fn restore(&mut self, s: &Snapshot) {
        self.text = s.text.clone();
        self.anchor = s.anchor;
        self.head = s.head;
        self.last_edit = None;
        self.multi = false;
        self.version += 1;
    }
    /// Replace the whole document. `version` keeps growing across loads so that
    /// results of background jobs from the previous document can never look
    /// "newer" than the current one (C6).
    pub fn load(&mut self, text: String) {
        let v = self.version + 1;
        *self = Document::new(text);
        self.version = v;
        self.saved_version = v;
    }
}
