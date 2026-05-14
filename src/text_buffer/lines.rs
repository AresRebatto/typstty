use super::super::debug_log::log;

use super::super::{
    cursor_repositioning, erease_current_line, rerender_current_line,
    rerender_lines_from_current_position,
};
use std::io::{Cursor, Stdout, Write, stdout};

pub struct Lines {
    lines: Vec<String>,
    actual_line: usize,

    /// Column offset of the cursor. Starts at 2 because column 0 is the
    /// tilde gutter and column 1 is the separator space.
    cursor_position: (u16, u16),
}

impl Lines {
    pub fn new() -> Self {
        let mut ob = Self {
            lines: Vec::new(),
            actual_line: 0,
            cursor_position: (2, 0),
        };

        ob.lines.push(String::new()); //The first line

        ob
    }

    pub fn x(&self) -> u16 {
        self.cursor_position.0
    }

    pub fn y(&self) -> u16 {
        self.cursor_position.1
    }

    /// Inserts a character at the current cursor position (printable key behavior).
    ///
    /// If the cursor is at the end of the line, the character is appended directly.
    /// Otherwise it is inserted at the cursor and the rest of the line is re-rendered
    /// to fill the resulting gap. The cursor advances by one column in both cases.
    /// Horizontal overflow is not yet handled (TODO).
    pub fn push_char(&mut self, c: char, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.is_eol() {
            // If this is the first character on an empty line, stamp the line
            // number in the gutter before writing the character itself.
            if self.is_current_line_empty() {
                self.write_line_number(stdout)?;
            }

            // Append the character to the internal buffer and echo it to the terminal.
            self.lines[self.actual_line].push(c);
            write!(stdout, "{c}")?;
            stdout.flush()?;
        } else {
            // Insert the character mid-line and rerender to shift trailing text right.
            self.lines[self.actual_line].insert((self.cursor_position.0 - 2) as usize, c);
            rerender_current_line!(stdout, self.cursor_position.1, self);
        }

        // Advance the cursor past the newly inserted character.
        self.cursor_position.0 += 1;
        cursor_repositioning!(stdout, self.cursor_position);

        // TODO: handle the case where the inserted character would overflow the terminal width.
        Ok(())
    }

    /// Removes the character immediately before the cursor (backspace behavior).
    ///
    /// Handles three distinct cases:
    /// - Cursor is mid-line: removes the character and rerenders the current line.
    /// - Cursor is at end of line: pops the last character and clears the trailing cell.
    /// - Cursor is at column 2 (line start): merges the current line into the previous one,
    ///   if this is the last line; otherwise the operation is a no-op (TODO).
    pub fn pop_char(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() == 2 {
            return self.merge_with_previous_line(stdout);
        }

        if self.is_eol() {
            // Cursor is at the end of the line: pop the last character,
            // move the cursor back, and overwrite the vacated cell with a space.
            self.lines[self.actual_line].pop();
            self.cursor_position.0 -= 1;
            cursor_repositioning!(stdout, self.cursor_position);
            write!(stdout, " ")?;
            stdout.flush()?;
            cursor_repositioning!(stdout, self.cursor_position);

            return Ok(());
        }

        // Cursor is in the middle of the line: remove the character to the
        // left of the cursor and rerender the line to close the resulting gap.
        self.lines[self.actual_line].remove((self.cursor_position.0 - 3) as usize);
        self.cursor_position.0 -= 1;
        rerender_current_line!(stdout, self.cursor_position.1, self);
        cursor_repositioning!(stdout, self.cursor_position);

        Ok(())
    }

    /// Inserts a newline at the current cursor position (Enter key behavior).
    ///
    /// If the cursor is at the end of the line, a new empty line is inserted below.
    /// When not at EOF, all subsequent lines are re-rendered to shift them down.
    /// Mid-line splitting is not yet implemented (TODO).
    /// Horizontal overflow is not yet handled (TODO).
    pub fn newline(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.is_eol() {
            // If the current line is empty, stamp its line number on the gutter
            // before opening the new line below it.
            if self.is_current_line_empty() {
                self.write_line_number(stdout)?;
            }

            if self.is_eof() {
                // Cursor is on the last line: simply append a new empty line.
                self.lines.push(String::new());
            } else {
                // Cursor is in the middle of the buffer: insert the new line and
                // rerender every following line to push them one row down.
                self.lines.insert(self.actual_line + 1, String::new());
                rerender_lines_from_current_position!(stdout, self);
            }

            // Move the cursor to the beginning of the newly created line.
            self.cursor_position.1 += 1;
            self.cursor_position.0 = 2;
            self.actual_line += 1;
            cursor_repositioning!(stdout, self.cursor_position);
        } else {
            // TODO: split the current line at the cursor position and move the
            // trailing text to the new line below.
        }

        // TODO: handle the case where the new line would overflow the terminal height.
        Ok(())
    }

    /// Moves the cursor one position to the left (← key behavior).
    ///
    /// If the cursor is already at the start of the line, wraps to the end of
    /// the previous line (unless we are on the very first line).
    pub fn left(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() == 2 && self.y() > 0 {
            // Wrap to the end of the previous line.
            self.actual_line -= 1;
            self.cursor_position.1 -= 1;
            self.cursor_position.0 = self.end_current_line() + 2;
            cursor_repositioning!(stdout, self.cursor_position);
        } else if self.x() > 2 {
            // Normal left movement within the same line.
            self.cursor_position.0 -= 1;
            cursor_repositioning!(stdout, self.cursor_position);
        }
        Ok(())
    }

    /// Moves the cursor one position to the right (→ key behavior).
    ///
    /// If the cursor is already at the end of the line, wraps to the beginning
    /// of the next line (unless we are on the last non-empty line).
    pub fn right(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() < self.end_current_line() + 2 {
            // Normal right movement within the same line.
            self.cursor_position.0 += 1;
            cursor_repositioning!(stdout, self.cursor_position);
        } else if self.is_eol()
            && self.end_current_line() != 0
            && self.actual_line + 1 < self.lines.len()
        {
            // Wrap to the beginning of the next line.
            self.cursor_position.0 = 2;
            self.cursor_position.1 += 1;
            self.actual_line += 1;
            cursor_repositioning!(stdout, self.cursor_position);
        }
        Ok(())
    }

    //---------------------------------------------------
    // SUPPORT FUNCTIONS
    //---------------------------------------------------

    fn merge_with_previous_line(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.is_eof() && self.lines.len() > 1 {
            // Cursor is at the very beginning of the last line: merge this line
            // into the previous one and remove the now-empty line entry.
            cursor_repositioning!(stdout, (0, self.y()));
            write!(stdout, "~")?;

            // Remember where the previous line ended so we can restore the cursor there.
            let x_coordinate = (self.lines[self.actual_line - 1].len() + 2) as u16;

            erease_current_line!(stdout, self.cursor_position.1, self);
            let popped = self.lines.pop().unwrap();
            self.lines[self.actual_line - 1].push_str(popped.as_str());

            // Move up to the previous line, placing the cursor at the merge point.
            self.cursor_position.1 -= 1;
            self.cursor_position.0 = x_coordinate;
            self.actual_line -= 1;
            rerender_current_line!(stdout, self.cursor_position.1, self);
            cursor_repositioning!(stdout, self.cursor_position);
        } else {
            self.actual_line -= 1;
            self.cursor_position.1 -= 1;
            let new_x = self.end_current_line() + 2;
            let current_line = self.lines[self.actual_line + 1].clone();
            self.lines[self.actual_line].push_str(&current_line);

            self.lines.remove(self.actual_line + 1);

            rerender_lines_from_current_position!(stdout, self);

            cursor_repositioning!(stdout, (0, self.lines.len() as u16));
            write!(stdout, "~")?;
            erease_current_line!(stdout, self.lines.len() as u16, self);

            self.cursor_position.0 = new_x;
            cursor_repositioning!(stdout, self.cursor_position);
        }

        return Ok(());
    }

    fn end_current_line(&self) -> u16 {
        return self.lines[self.actual_line].len() as u16;
    }

    fn write_line_number(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        cursor_repositioning!(stdout, (0, self.y()));
        let n_line = self.y();
        write!(stdout, "{n_line}")?;
        stdout.flush()?;
        cursor_repositioning!(stdout, self.cursor_position);

        return Ok(());
    }

    //---------------------------------------------------
    //EXTRACTING CONDITIONS
    //---------------------------------------------------

    ///Check if the cursor is at the end of the current line
    #[inline]
    fn is_eol(&self) -> bool {
        self.x() - 2 == self.end_current_line()
    }

    ///Check if the cursor is at the start of the current line
    #[inline]
    fn is_current_line_empty(&self) -> bool {
        self.end_current_line() == 0
    }

    #[inline]
    fn is_eof(&self) -> bool {
        self.actual_line + 1 == self.lines.len()
    }
}
