use eframe::egui::{self, Color32, FontFamily, FontId, Key, Modifiers, Pos2, Rect, Stroke, Vec2};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use crate::text_buffer::lines::Lines;

// ─── Palette ────────────────────────────────────────────────────────────────

const BG: Color32 = Color32::from_rgb(30, 30, 46); // base
const GUTTER_BG: Color32 = Color32::from_rgb(24, 24, 37); // mantle
const LINE_NUM: Color32 = Color32::from_rgb(88, 91, 112); // overlay0
const LINE_NUM_ACTIVE: Color32 = Color32::from_rgb(166, 173, 200); // text
const TEXT_COLOR: Color32 = Color32::from_rgb(205, 214, 244); // text
const CURSOR_COLOR: Color32 = Color32::from_rgb(137, 180, 250); // blue
const SELECTION_COLOR: Color32 = Color32::from_rgba_premultiplied(137, 180, 250, 40);
const CURRENT_LINE_HL: Color32 = Color32::from_rgba_premultiplied(49, 50, 68, 180); // surface0

const FONT_SIZE: f32 = 15.0;
const GUTTER_WIDTH: f32 = 48.0;
const LINE_PADDING: f32 = 2.0; // extra vertical padding per line
const H_PADDING: f32 = 8.0; // left padding after gutter

// ─── App ────────────────────────────────────────────────────────────────────

pub struct TypsttyApp {
    buffer: Lines,
    file_path: PathBuf,

    /// Accumulated blink phase (seconds). The cursor blinks every 0.5 s.
    blink_acc: f32,
    cursor_visible: bool,
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
        }
    }

    // ── Font setup ──────────────────────────────────────────────────────────

    fn configure_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

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

    // ── Save ────────────────────────────────────────────────────────────────

    fn save(&self) {
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

    // ── Input handling ──────────────────────────────────────────────────────

    /// Process all keyboard events that arrived this frame.
    /// Returns `true` if the buffer was modified (to reset the cursor blink).
    fn handle_input(&mut self, ctx: &egui::Context) -> bool {
        let mut modified = false;

        ctx.input_mut(|i| {
            // ── Ctrl shortcuts ────────────────────────────────────────────
            if i.consume_key(Modifiers::CTRL, Key::S) {
                self.save();
            }
            // TODO Ctrl-Z / Ctrl-Shift-Z (undo/redo) could be wired here later.

            // ── Navigation ────────────────────────────────────────────────
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
            }

            // ── Editing ───────────────────────────────────────────────────
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
                    // Delete = move right then backspace (delete char under cursor).
                    let old_col = self.buffer.col();
                    let old_row = self.buffer.row();
                    self.buffer.move_right();
                    if self.buffer.col() != old_col || self.buffer.row() != old_row {
                        self.buffer.pop_char();
                        modified = true;
                    }
                }

                // ── Printable characters ──────────────────────────────────────
                for event in &i.events {
                    if let egui::Event::Text(text) = event {
                        for c in text.chars() {
                            self.buffer.push_char(c);
                        }
                        modified = true;
                    }
                }

                // Tab → insert 4 spaces (common for .typ files)
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

        // Background
        painter.rect_filled(available, 0.0, BG);

        // Gutter background
        let gutter_rect =
            Rect::from_min_size(available.min, Vec2::new(GUTTER_WIDTH, available.height()));
        painter.rect_filled(gutter_rect, 0.0, GUTTER_BG);

        let cursor_row = self.buffer.row();
        let cursor_col = self.buffer.col();
        let lines = self.buffer.lines().to_vec(); // clone to avoid borrow issues

        for (row_idx, line_text) in lines.iter().enumerate() {
            let y = available.min.y + row_idx as f32 * line_h;

            // ── Current line highlight ───────────────────────────────────
            if row_idx == cursor_row {
                let hl_rect = Rect::from_min_size(
                    Pos2::new(available.min.x + GUTTER_WIDTH, y),
                    Vec2::new(available.width() - GUTTER_WIDTH, line_h),
                );
                painter.rect_filled(hl_rect, 0.0, CURRENT_LINE_HL);
            }

            // ── Line number ──────────────────────────────────────────────
            let num_str = (row_idx + 1).to_string();
            let num_color = if row_idx == cursor_row {
                LINE_NUM_ACTIVE
            } else {
                LINE_NUM
            };
            let num_x = available.min.x + GUTTER_WIDTH - H_PADDING;
            painter.text(
                Pos2::new(num_x, y + LINE_PADDING),
                egui::Align2::RIGHT_TOP,
                &num_str,
                FontId::new(FONT_SIZE, FontFamily::Monospace),
                num_color,
            );

            // ── Line text ────────────────────────────────────────────────
            let text_x = available.min.x + GUTTER_WIDTH + H_PADDING;
            painter.text(
                Pos2::new(text_x, y + LINE_PADDING),
                egui::Align2::LEFT_TOP,
                line_text,
                FontId::new(FONT_SIZE, FontFamily::Monospace),
                TEXT_COLOR,
            );

            // ── Cursor ───────────────────────────────────────────────────
            if row_idx == cursor_row && self.cursor_visible {
                let byte_idx = line_text
                    .char_indices()
                    .nth(cursor_col)
                    .map(|(i, _)| i)
                    .unwrap_or(line_text.len());
                let text_before_cursor = &line_text[..byte_idx];
                let cursor_x = text_x
                    + ctx
                        .fonts(|f| {
                            f.layout_no_wrap(
                                text_before_cursor.to_owned(),
                                FontId::new(FONT_SIZE, FontFamily::Monospace),
                                TEXT_COLOR,
                            )
                        })
                        .size()
                        .x;
                let cursor_rect = Rect::from_min_size(
                    Pos2::new(cursor_x, y + LINE_PADDING),
                    Vec2::new(2.0, line_h - LINE_PADDING * 2.0),
                );
                painter.rect_filled(cursor_rect, 0.0, CURSOR_COLOR);
            }
        }

        // Reserve the space so egui knows we painted there.
        ui.allocate_rect(available, egui::Sense::click());
    }
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
