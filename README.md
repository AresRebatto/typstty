# Typst Live

A single-surface live Typst editor in Rust (egui/eframe 0.37 + Typst 0.15.1).
Typst source is the only document representation. Blocks you are not editing are
typeset in place by the real Typst compiler; the block containing the caret shows
its editable, highlighted source. No preview pane.

## Build & run
Requires Rust >= 1.89 (tested with 1.99.1) and, on Linux, the usual X11/Wayland
libraries for eframe.

    cargo run --release            # empty welcome doc
    cargo run --release -- file.typ
    cargo test                     # model, compiler, files and settings tests

Pinned: typst/typst-render/typst-pdf/typst-syntax/typst-layout/typst-assets 0.15.1,
eframe 0.37.0, rfd 0.17 (see Cargo.lock).

## Architecture
- `src/commands.rs` – pure shortcut commands on (text, byte selection). The Typst
  syntax tree decides if text is already Strong/Emph/Raw/Equation.
- `src/doc.rs` – source string, selection, undo/redo (typing bursts coalesced),
  block segmentation via `typst_syntax::parse`, active-block resolution.
- `src/engine.rs` – `World` implementation, per-block compile + raster, whole
  document compile (diagnostics), PDF export.
- `src/main.rs` – egui UI: block surface, toolbar, find/replace, outline,
  diagnostics panel, settings, file dialogs, worker threads.
Rendering never rewrites source; an edit only replaces the active block's byte range.

## Shortcuts
Ctrl/Cmd+B bold `*..*`, Ctrl/Cmd+I italic `_.._`, Alt+C inline raw, Alt+Shift+C
fenced raw block, Alt+M inline math `$x$`, Alt+Shift+M display math `$ x $`.
Pressing again removes the formatting. Also: Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z,
Ctrl+S / Ctrl+Shift+S / Ctrl+O / Ctrl+N / Ctrl+F (or H).

## Supported constructs
Everything Typst supports, because it is compiled by Typst. Blocks that fail to
compile are shown as their source in red with the error on hover.

## Selection, merging and navigation
- Ctrl+A selects the block; pressed again it selects the whole document. Shift+arrows
  at a block edge, Ctrl+Shift+Home/End and Shift+click extend the selection across
  blocks; typing, Backspace/Delete, copy, cut and paste then act on the whole range.
- Backspace at the start of a block (Delete at the end) removes the gap between
  blocks and joins them (headings and `#set/#let/...` blocks are never fused); one
  undo step restores text and caret.
- Ctrl+Home / Ctrl+End jump to the document start / end.

## Files and safety
- Saves are atomic (temp file + fsync + rename); CRLF line endings and a UTF-8 BOM are
  preserved byte for byte; saving over a file changed on disk asks first.
- A recovery snapshot (`.<name>.recovery`) is written next to the file after 3 s of
  inactivity and offered on the next open; it is deleted by a successful save.
- Settings and the last opened file persist in the user config dir
  (`typst-live/settings.txt`).
- Worker threads catch panics inside Typst; a pending render that never answers
  expires after 30 s.

## Keyboard layouts (AltGr)
`vendor/egui-winit` is egui-winit 0.37.0 with one patch: Ctrl+Alt (AltGr on Windows)
no longer discards typed text, so `{ } [ ] @ # \\ |` work on IT/ES/DE layouts. Alt+C /
Alt+M are matched on the physical key (macOS Option) and ignore AltGr.

## Limitations
- Each block is compiled separately (with preceding `#set/#show/#let/#import`
  blocks): cross-block `@label` references, counters and heading numbering are not
  reflected in the in-place rendering (diagnostics and PDF export are correct).
  Multi-page blocks are stacked (up to 12 pages).
- Clicking a rendered block maps the click to the nearest source line/column
  proportionally (approximate); links are not followed.
- Inline live formatting in the active block hides the delimiters of `*..*`, `_.._`
  and inline `$..$` when the caret is not within one byte of them and applies
  bold/italic; inline math is not typeset there (it is in rendered blocks).
- Fonts are the Typst-embedded ones; `@preview` packages are not resolved.
- Not implemented from the report: vault (U5), design tokens/themes (U6), whole-document
  compile with block-to-region map (C2), system fonts/packages (C3), pixel-exact
  click mapping (C9), link following (C8), file watcher, split of `main.rs` (Q1).
- The egui layer is smoke-tested under Xvfb only; AltGr/macOS key handling and IME are
  untested on real hardware.
