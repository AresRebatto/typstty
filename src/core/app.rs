use eframe::App;
use eframe::egui::{self, Color32, FontFamily, FontId, Key, Modifiers, Pos2, Rect, Stroke, Vec2};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, exit};

use super::app_snapshot::AppSnapshot;
use super::highlight_span::*;
use crate::text_buffer::lines::Lines;
// ─── Palette ────────────────────────────────────────────────────────────────

const BG: Color32 = Color32::from_rgb(30, 30, 46); // base
const GUTTER_BG: Color32 = Color32::from_rgb(24, 24, 37); // mantle
const LINE_NUM: Color32 = Color32::from_rgb(88, 91, 112); // overlay0
const LINE_NUM_ACTIVE: Color32 = Color32::from_rgb(166, 173, 200); // text
const TEXT_COLOR: Color32 = Color32::from_rgb(205, 214, 244); // text
const CURSOR_COLOR: Color32 = Color32::from_rgb(137, 180, 250); // blue
const CURRENT_LINE_HL: Color32 = Color32::from_rgba_premultiplied(49, 50, 68, 180); // surface0

const FONT_SIZE: f32 = 15.0;
const GUTTER_WIDTH: f32 = 48.0;
const LINE_PADDING: f32 = 2.0; // extra vertical padding per line
const H_PADDING: f32 = 8.0; // left padding after gutter

const MAX_SNAPSHOTS: usize = 20;
// ─── App ────────────────────────────────────────────────────────────────────
pub struct TypsttyApp {
    buffer: Lines,
    file_path: PathBuf,

    /// Accumulated blink phase (seconds). The cursor blinks every 0.5 s.
    blink_acc: f32,
    cursor_visible: bool,

    snapshot: Vec<AppSnapshot>,
    undo_snapshot: Vec<AppSnapshot>,
}

impl TypsttyApp {
    pub fn new(cc: &eframe::CreationContext<'_>, file_path: PathBuf) -> Self {
        Self::configure_fonts(&cc.egui_ctx);

        let mut buffer = Lines::new();
        if file_path.exists() {
            if let Ok(mut f) = File::open(&file_path) {
                let mut content = String::new();
                let _ = f.read_to_string(&mut content);
                buffer.load_from_str(&content);
            }
        }

        Self {
            buffer,
            file_path,
            blink_acc: 0.0,
            cursor_visible: true,

            snapshot: Vec::new(),
            undo_snapshot: Vec::new(),
        }
    }

    fn push_snapshot(&mut self) {
        if self.snapshot.len() >= MAX_SNAPSHOTS {
            self.snapshot.remove(0);
        }
        self.snapshot.push(AppSnapshot {
            buffer: self.buffer.clone(),
        });
    }

    fn undo(&mut self) {
        if let Some(snap) = self.snapshot.pop() {
            self.undo_snapshot.push(AppSnapshot {
                buffer: self.buffer.clone(),
            });
            self.buffer = snap.buffer;
        }
    }

    fn redo(&mut self) {
        if let Some(snap) = self.undo_snapshot.pop() {
            self.snapshot.push(AppSnapshot {
                buffer: self.buffer.clone(),
            });
            self.buffer = snap.buffer;
        }
    }
    
    // ── Font setup ──────────────────────────────────────────────────────────

    fn configure_fonts(ctx: &egui::Context) {
        let fonts = egui::FontDefinitions::default();

        ctx.set_fonts(fonts);

        let mut style = (*ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            FontId::new(FONT_SIZE, FontFamily::Monospace),
        );
        // Make the default body style also use a slightly larger monospace font.
        style.text_styles.insert(
            egui::TextStyle::Body,
            FontId::new(FONT_SIZE, FontFamily::Monospace),
        );
        ctx.set_style(style);
    }

    // ── Save and Compile ────────────────────────────────────────────────────────────────

    fn save_to_file(&self) {
        let result = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.file_path)
            .and_then(|mut f| self.buffer.save(&mut f));

        if let Err(e) = result {
            eprintln!("Save error: {e}");
        }
    }

    fn compile_typst(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.save_to_file();

        //TODO use typst-pdf
        let status = Command::new("typst")
            .args([
                "compile",
                self.file_path.to_str().unwrap(),
                &format!("{}.pdf", self.file_path.to_str().unwrap()),
            ])
            .status()?;

        if !status.success() {
            return Err("typst compile failed".into());
        }
        Ok(())
    }

    // ── Input handling ──────────────────────────────────────────────────────

    /// Process all keyboard events that arrived this frame.
    /// Returns `true` if the buffer was modified (to reset the cursor blink).
    fn handle_input(&mut self, ctx: &egui::Context) -> bool {
        let mut modified = false;
    
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::CTRL, Key::S) {
                self.save_to_file();
            }
            if i.consume_key(Modifiers::CTRL, Key::Z) {
                self.undo();
                return; // non pushare snapshot dopo un undo
            }
            if i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Z) {
                self.redo();
                return;
            }
    
            // Snapshot PRIMA della modifica
            let mut will_modify = false;
    
            if i.key_pressed(Key::Enter)
                || i.key_pressed(Key::Backspace)
                || i.key_pressed(Key::Delete)
                || i.key_pressed(Key::Tab)
            {
                will_modify = true;
            }
            for event in &i.events {
                if matches!(event, egui::Event::Text(_)) {
                    will_modify = true;
                }
            }
    
            if will_modify {
                self.push_snapshot();
                // svuota undo quando l'utente modifica manualmente
                self.undo_snapshot.clear();
            }
    
            // Navigation
            if i.consume_key(Modifiers::CTRL, Key::ArrowRight) {
                self.buffer.move_ctrl_right();
            } else if i.consume_key(Modifiers::CTRL, Key::ArrowLeft) {
                self.buffer.move_ctrl_left();
            } else if i.key_pressed(Key::ArrowLeft) {
                self.buffer.move_left();
            } else if i.key_pressed(Key::ArrowRight) {
                self.buffer.move_right();
            } else if i.key_pressed(Key::ArrowUp) {
                self.buffer.move_up();
            } else if i.key_pressed(Key::ArrowDown) {
                self.buffer.move_down();
            } else if i.key_pressed(Key::Home) {
                self.buffer.move_home();
            } else if i.key_pressed(Key::End) {
                self.buffer.move_end();
            } else if i.consume_key(Modifiers::CTRL, Key::Backspace) {
                self.buffer.pop_word();
            } else if i.key_pressed(Key::Q) && i.modifiers.ctrl {
                exit(0);
            } else if i.key_pressed(Key::R) && i.modifiers.ctrl {
                if let Err(e) = self.compile_typst() {
                    panic!("Error: {e}");
                }
            }
    
            if !i.modifiers.ctrl {
                if i.key_pressed(Key::Enter) {
                    self.buffer.newline();
                    modified = true;
                }
                if i.key_pressed(Key::Backspace) {
                    self.buffer.pop_char();
                    modified = true;
                }
                if i.key_pressed(Key::Delete) {
                    let old_col = self.buffer.col();
                    let old_row = self.buffer.row();
                    self.buffer.move_right();
                    if self.buffer.col() != old_col || self.buffer.row() != old_row {
                        self.buffer.pop_char();
                        modified = true;
                    }
                }
    
                for event in &i.events {
                    if let egui::Event::Text(text) = event {
                        for c in text.chars() {
                            self.buffer.push_char(c);
                            if c == ' ' {
                                modified = true;
                            }
                        }
                    }
                }
    
                if i.key_pressed(Key::Tab) {
                    for _ in 0..4 {
                        self.buffer.push_char(' ');
                    }
                    modified = true;
                }
            }
        });
    
        modified
    }

    // -- Rendering ───────────────────────────────────────────────────────────

    fn paint_editor(&mut self, ui: &mut egui::Ui, char_w: f32, ctx: &egui::Context, line_h: f32) {
        let available = ui.available_rect_before_wrap();
        let painter = ui.painter_at(available);

        self.paint_backgrounds(&painter, available);

        let full_text = self.buffer.full_text();
        let spans = compute_highlight(&full_text);
        let line_offsets = self.buffer.line_byte_offsets();
        let cursor_row = self.buffer.row();
        let cursor_col = self.buffer.col();
        let lines = self.buffer.lines().to_vec();

        for (row_idx, line_text) in lines.iter().enumerate() {
            let y = available.min.y + row_idx as f32 * line_h;
            let text_x = available.min.x + GUTTER_WIDTH + H_PADDING;

            if row_idx == cursor_row {
                self.paint_current_line_highlight(&painter, available, y, line_h);
            }

            self.paint_line_number(&painter, available, y, row_idx, cursor_row);

            let segments = build_segments(line_text, row_idx, &line_offsets, &spans);
            self.paint_segments(ctx, &painter, &segments, text_x, y);

            if row_idx == cursor_row && self.cursor_visible {
                self.paint_cursor(ctx, &painter, line_text, cursor_col, text_x, y, line_h);
            }
        }

        ui.allocate_rect(available, egui::Sense::click());
    }

    fn paint_backgrounds(&self, painter: &egui::Painter, available: Rect) {
        painter.rect_filled(available, 0.0, BG);
        let gutter_rect =
            Rect::from_min_size(available.min, Vec2::new(GUTTER_WIDTH, available.height()));
        painter.rect_filled(gutter_rect, 0.0, GUTTER_BG);
    }

    fn paint_current_line_highlight(
        &self,
        painter: &egui::Painter,
        available: Rect,
        y: f32,
        line_h: f32,
    ) {
        let hl_rect = Rect::from_min_size(
            Pos2::new(available.min.x + GUTTER_WIDTH, y),
            Vec2::new(available.width() - GUTTER_WIDTH, line_h),
        );
        painter.rect_filled(hl_rect, 0.0, CURRENT_LINE_HL);
    }

    fn paint_line_number(
        &self,
        painter: &egui::Painter,
        available: Rect,
        y: f32,
        row_idx: usize,
        cursor_row: usize,
    ) {
        let color = if row_idx == cursor_row {
            LINE_NUM_ACTIVE
        } else {
            LINE_NUM
        };
        painter.text(
            Pos2::new(available.min.x + GUTTER_WIDTH - H_PADDING, y + LINE_PADDING),
            egui::Align2::RIGHT_TOP,
            &(row_idx + 1).to_string(),
            FontId::new(FONT_SIZE, FontFamily::Monospace),
            color,
        );
    }

    fn paint_segments(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        segments: &[(&str, Color32)],
        text_x: f32,
        y: f32,
    ) {
        let mut x = text_x;
        for (text, color) in segments {
            if text.is_empty() {
                continue;
            }
            let galley = ctx.fonts(|f| {
                f.layout_no_wrap(
                    text.to_string(),
                    FontId::new(FONT_SIZE, FontFamily::Monospace),
                    *color,
                )
            });
            painter.galley(Pos2::new(x, y + LINE_PADDING), galley.clone(), *color);
            x += galley.size().x;
        }
    }

    fn paint_cursor(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        line_text: &str,
        cursor_col: usize,
        text_x: f32,
        y: f32,
        line_h: f32,
    ) {
        let byte_idx = line_text
            .char_indices()
            .nth(cursor_col)
            .map(|(i, _)| i)
            .unwrap_or(line_text.len());
        let width = ctx
            .fonts(|f| {
                f.layout_no_wrap(
                    line_text[..byte_idx].to_owned(),
                    FontId::new(FONT_SIZE, FontFamily::Monospace),
                    TEXT_COLOR,
                )
            })
            .size()
            .x;
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(text_x + width, y + LINE_PADDING),
                Vec2::new(2.0, line_h - LINE_PADDING * 2.0),
            ),
            0.0,
            CURSOR_COLOR,
        );
    }
}

fn build_segments<'a>(
    line_text: &'a str,
    row_idx: usize,
    line_offsets: &[usize],
    spans: &'a [HighlightSpan],
) -> Vec<(&'a str, Color32)> {
    let line_start = line_offsets[row_idx];
    let line_end = line_start + line_text.len();

    let mut row_spans: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.byte_range.start < line_end && s.byte_range.end > line_start)
        .collect();
    row_spans.sort_by_key(|s| s.byte_range.start);

    let mut segments = Vec::new();
    let mut cursor_in_line = 0usize;

    for span in &row_spans {
        let local_start = span
            .byte_range
            .start
            .saturating_sub(line_start)
            .min(line_text.len());
        let local_end = span
            .byte_range
            .end
            .saturating_sub(line_start)
            .min(line_text.len());

        if cursor_in_line < local_start {
            segments.push((&line_text[cursor_in_line..local_start], TEXT_COLOR));
        }
        if local_start < local_end {
            segments.push((&line_text[local_start..local_end], span.color));
        }
        cursor_in_line = cursor_in_line.max(local_end);
    }

    if cursor_in_line < line_text.len() {
        segments.push((&line_text[cursor_in_line..], TEXT_COLOR));
    }
    if segments.is_empty() {
        segments.push((line_text, TEXT_COLOR));
    }

    segments
}

// ─── eframe::App impl ───────────────────────────────────────────────────────

impl eframe::App for TypsttyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Blink the cursor at 2 Hz (visible 0.5 s, hidden 0.5 s).
        let dt = ctx.input(|i| i.unstable_dt);
        self.blink_acc += dt;
        if self.blink_acc >= 0.5 {
            self.blink_acc -= 0.5;
            self.cursor_visible = !self.cursor_visible;
        }

        let modified = self.handle_input(ctx);
        if modified {
            // Reset blink so the cursor is always visible right after a keystroke.
            self.blink_acc = 0.0;
            self.cursor_visible = true;
        }

        // Derive character dimensions from the current font metrics.
        // egui doesn't expose this directly, so we measure a reference glyph.
        let char_w = {
            let galley = ctx.fonts(|f| {
                f.layout_no_wrap(
                    "M".to_owned(),
                    FontId::new(FONT_SIZE, FontFamily::Monospace),
                    TEXT_COLOR,
                )
            });
            galley.size().x
        };
        let line_h = FONT_SIZE + LINE_PADDING * 2.0 + 2.0;

        // Title bar — show filename and a dirty indicator (future: track dirty state).
        let title = format!("typstty — {}", self.file_path.display());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // ── Top bar ─────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("topbar")
            .frame(
                egui::Frame::none()
                    .fill(GUTTER_BG)
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("typstty")
                            .color(CURSOR_COLOR)
                            .font(FontId::new(FONT_SIZE, FontFamily::Monospace))
                            .strong(),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            self.file_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("untitled"),
                        )
                        .color(LINE_NUM_ACTIVE)
                        .font(FontId::new(FONT_SIZE, FontFamily::Monospace)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let pos_label = format!(
                            "Ln {}, Col {}",
                            self.buffer.row() + 1,
                            self.buffer.col() + 1
                        );
                        ui.label(
                            egui::RichText::new(pos_label)
                                .color(LINE_NUM)
                                .font(FontId::new(FONT_SIZE - 1.0, FontFamily::Monospace)),
                        );
                    });
                });
            });

        // ── Status bar ──────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("statusbar")
            .frame(
                egui::Frame::none()
                    .fill(GUTTER_BG)
                    .inner_margin(egui::Margin::symmetric(8.0, 3.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Ctrl+S  Save")
                            .color(LINE_NUM)
                            .font(FontId::new(FONT_SIZE - 2.0, FontFamily::Monospace)),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new("Ctrl+Q  Quit")
                            .color(LINE_NUM)
                            .font(FontId::new(FONT_SIZE - 2.0, FontFamily::Monospace)),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new("Ctrl+R  Compile")
                            .color(LINE_NUM)
                            .font(FontId::new(FONT_SIZE - 2.0, FontFamily::Monospace)),
                    );
                });
            });

        // ── Main editor area ────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        self.paint_editor(ui, char_w, ctx, line_h);
                    });
            });

        // Request repaint for the cursor blink.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
