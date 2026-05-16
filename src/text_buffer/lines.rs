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
        let byte_idx = Self::char_to_byte_offset(&self.lines[self.actual_line], col);
        self.lines[self.actual_line].insert(byte_idx, c);
        self.cursor.0 += 1;
    }

    pub fn pop_char(&mut self) {
        if self.col() == 0 {
            self.merge_with_previous_line();
            return;
        }
        let col = self.col();
        let byte_idx = Self::char_to_byte_offset(&self.lines[self.actual_line], col - 1);
        self.lines[self.actual_line].remove(byte_idx);
        self.cursor.0 -= 1;
    }
    
    pub fn pop_word(&mut self) {
        if self.col() == 0 {
            self.merge_with_previous_line();
            return;
        }
    
        let target = self.prev_word_boundary(self.col());
        let chars_to_remove = self.col() - target;
    
        for _ in 0..chars_to_remove {
            let col = self.col();
            let byte_idx = Self::char_to_byte_offset(&self.lines[self.actual_line], col - 1);
            self.lines[self.actual_line].remove(byte_idx);
            self.cursor.0 -= 1;
        }
    }
    
    /// Enter: split the current line at the cursor, push the tail onto a new line.
    pub fn newline(&mut self) {
        let col = self.col();
        let byte_idx = Self::char_to_byte_offset(&self.lines[self.actual_line], col);
        let tail = self.lines[self.actual_line].split_off(byte_idx);

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

    pub fn move_ctrl_right(&mut self) {
        let line_char_len = self.lines[self.actual_line].chars().count();

        if self.col() < line_char_len {
            let current_col = self.col();
            let new_col = self.next_word_boundary(current_col, line_char_len);
            self.cursor.0 = new_col;
        } else if !self.is_eof() {
            self.actual_line += 1;
            self.cursor.1 += 1;
            self.cursor.0 = 0;
        }
    }
    
    pub fn move_ctrl_left(&mut self) {
        if self.col() > 0 {
            let current_col = self.col();
            self.cursor.0 = self.prev_word_boundary(current_col);
        } else if self.actual_line > 0 {
            self.actual_line -= 1;
            self.cursor.1 -= 1;
            self.cursor.0 = self.current_line_len();
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
        self.lines[self.actual_line].chars().count()
    }

    #[inline]
    fn is_eof(&self) -> bool {
        self.actual_line + 1 == self.lines.len()
    }

    fn merge_with_previous_line(&mut self) {
        if self.actual_line == 0 {
            return;
        }
        let current = self.lines.remove(self.actual_line);
        self.actual_line -= 1;
        self.cursor.1 -= 1;

        let prev_char_count = self.lines[self.actual_line].chars().count();
        self.cursor.0 = prev_char_count;
        self.lines[self.actual_line].push_str(&current);
    }

    fn char_to_byte_offset(s: &str, char_offset: usize) -> usize {
        s.char_indices()
            .nth(char_offset)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(s.len())
    }

    fn next_word_boundary(&self, from_char: usize, line_char_len: usize) -> usize {
        let line = &self.lines[self.actual_line];
        let chars: Vec<(usize, char)> = line.char_indices().collect();

        // Skippa spazi sulla posizione corrente
        let after_spaces = chars
            .iter()
            .skip(from_char)
            .skip_while(|&&(_, c)| c.is_whitespace())
            .next()
            .map(|&(_, _)| ())
            .map(|_| {
                chars
                    .iter()
                    .skip(from_char)
                    .take_while(|&&(_, c)| c.is_whitespace())
                    .count()
            })
            .unwrap_or(0);

        let start = from_char + after_spaces;

        // Poi skippa la parola
        let word_len = chars
            .iter()
            .skip(start)
            .take_while(|&&(_, c)| !c.is_whitespace())
            .count();

        (start + word_len).min(line_char_len)
    }
    
    fn prev_word_boundary(&self, from_char: usize) -> usize {
        let line = &self.lines[self.actual_line];
        let chars: Vec<char> = line.chars().collect();
    
        let mut col = from_char;
    
        // Skip space immediatly left
        while col > 0 && chars[col - 1].is_whitespace() {
            col -= 1;
        }
        // Skip word characters
        while col > 0 && !chars[col - 1].is_whitespace() {
            col -= 1;
        }
    
        col
    }
}
