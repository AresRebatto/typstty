/// Pure text buffer — no terminal I/O, no stdout side-effects.
///
/// All rendering is delegated to the `egui` layer (`App`). This module only
/// owns the document state and the cursor, and exposes editing primitives.

#[derive(Debug, Clone)]
pub struct Lines {
    lines: Vec<String>,

    /// Index into `lines` of the row the cursor sits on.
    pub actual_line: usize,

    /// `(col, row)` in *character* space (0-based).
    /// `col` is the byte/char offset within the current line.
    pub cursor: (usize, usize),
}

impl Lines {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            actual_line: 0,
            cursor: (0, 0),
        }
    }

    // -----------------------------------------------------------------------
    // Public read-only accessors
    // -----------------------------------------------------------------------

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn col(&self) -> usize {
        self.cursor.0
    }

    pub fn row(&self) -> usize {
        self.cursor.1
    }

    // -----------------------------------------------------------------------
    // Editing primitives
    // -----------------------------------------------------------------------

    /// Insert `c` at the current cursor position and advance the cursor.
    pub fn push_char(&mut self, c: char) {
        let col = self.col();
        self.lines[self.actual_line].insert(col, c);
        self.cursor.0 += 1;
    }

    /// Backspace: remove the character immediately before the cursor.
    ///
    /// If the cursor is at the beginning of a line, merges the current line
    /// into the previous one (mirroring the original `merge_with_previous_line`).
    pub fn pop_char(&mut self) {
        if self.col() == 0 {
            self.merge_with_previous_line();
            return;
        }

        let col = self.col();
        self.lines[self.actual_line].remove(col - 1);
        self.cursor.0 -= 1;
    }

    /// Enter: split the current line at the cursor, push the tail onto a new line.
    pub fn newline(&mut self) {
        let col = self.col();
        let tail = self.lines[self.actual_line].split_off(col);

        if self.is_eof() {
            self.lines.push(tail);
        } else {
            self.lines.insert(self.actual_line + 1, tail);
        }

        self.actual_line += 1;
        self.cursor = (0, self.actual_line);
    }

    /// Move the cursor one character to the left, wrapping to the previous line.
    pub fn move_left(&mut self) {
        if self.col() > 0 {
            self.cursor.0 -= 1;
        } else if self.actual_line > 0 {
            self.actual_line -= 1;
            self.cursor.1 -= 1;
            self.cursor.0 = self.current_line_len();
        }
    }

    /// Move the cursor one character to the right, wrapping to the next line.
    pub fn move_right(&mut self) {
        if self.col() < self.current_line_len() {
            self.cursor.0 += 1;
        } else if !self.is_eof() {
            self.actual_line += 1;
            self.cursor.1 += 1;
            self.cursor.0 = 0;
        }
    }

    /// Move the cursor one row up, clamping the column to the new line length.
    pub fn move_up(&mut self) {
        if self.actual_line > 0 {
            self.actual_line -= 1;
            self.cursor.1 -= 1;
            self.cursor.0 = self.cursor.0.min(self.current_line_len());
        }
    }

    /// Move the cursor one row down, clamping the column to the new line length.
    pub fn move_down(&mut self) {
        if !self.is_eof() {
            self.actual_line += 1;
            self.cursor.1 += 1;
            self.cursor.0 = self.cursor.0.min(self.current_line_len());
        }
    }

    /// Jump the cursor to the beginning of the current line (Home key).
    pub fn move_home(&mut self) {
        self.cursor.0 = 0;
    }

    /// Jump the cursor to the end of the current line (End key).
    pub fn move_end(&mut self) {
        self.cursor.0 = self.current_line_len();
    }

    /// Serialize the buffer to a file (one `\n` per line).
    pub fn save(&self, file: &mut std::fs::File) -> std::io::Result<()> {
        use std::io::Write;
        for line in &self.lines {
            writeln!(file, "{line}")?;
        }
        Ok(())
    }

    /// Replace the entire buffer with `text` (used when opening an existing file).
    pub fn load_from_str(&mut self, text: &str) {
        self.lines = text.lines().map(str::to_owned).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.actual_line = 0;
        self.cursor = (0, 0);
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn current_line_len(&self) -> usize {
        self.lines[self.actual_line].len()
    }

    #[inline]
    fn is_eof(&self) -> bool {
        self.actual_line + 1 == self.lines.len()
    }

    fn merge_with_previous_line(&mut self) {
        if self.actual_line == 0 {
            return; // Nothing above to merge into.
        }

        let current = self.lines.remove(self.actual_line);
        self.actual_line -= 1;
        self.cursor.1 -= 1;

        // Place the cursor at the old end of the previous line before appending.
        let prev_len = self.lines[self.actual_line].len();
        self.cursor.0 = prev_len;

        self.lines[self.actual_line].push_str(&current);
    }
}