use crate::models::line;

use super::super::cursor_repositioning;
use super::line::*;
use std::io::{Cursor, Stdout, Write, stdout};

pub struct Lines {
    lines: Vec<Line>,
    actual_line: u16,
    pub cursor_position: (u16, u16),
}

impl Lines {
    pub fn new() -> Self {
        let mut ob = Self {
            lines: Vec::new(),
            actual_line: 0,
            cursor_position: (2, 0),
        };

        ob.lines.push(Line::new()); //The first line

        ob
    }

    pub fn x(&self) -> u16 {
        self.cursor_position.0
    }

    pub fn y(&self) -> u16 {
        self.cursor_position.1
    }

    pub fn push_char(&mut self, c: char, stdout: &mut Stdout) -> std::io::Result<()> {
        //Is he adding a character at the end of the line?
        if self.x() - 2 == self.end_current_line() {
            if self.lines[self.actual_line as usize].line.len() == 0 {
                cursor_repositioning!(stdout, (0, self.y()));
                let n_line = self.y();
                write!(stdout, "{n_line}")?;
                stdout.flush()?;
                cursor_repositioning!(stdout, self.cursor_position);
            }
            self.lines[self.actual_line as usize].push_ch(c);
            write!(stdout, "{c}")?;
            stdout.flush()?;
        } else {
            //TODO manage the character in the middle of line
        }

        //FIXME not a real bug, but this here with the inserting in the
        // middle of the line envolve in buggy things
        self.cursor_position.0 += 1;

        return Ok(());
    }

    pub fn pop_char(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() != 2 {
            if self.x() - 2 == self.end_current_line() {
                self.lines[self.actual_line as usize].pop_ch();
                self.cursor_position.0 -= 1;
                cursor_repositioning!(stdout, self.cursor_position);
                write!(stdout, " ")?;
                stdout.flush()?;
                cursor_repositioning!(stdout, self.cursor_position);
            } else {
                //TODO manage the character in the middle of line
            }
        } else {
            //TODO erase current line
        }

        return Ok(());
    }

    pub fn newline(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.end_current_line() == 0 {
            cursor_repositioning!(stdout, (0, self.y()));
            let n_line = self.y();
            write!(stdout, "{n_line}")?;
            stdout.flush()?;
            cursor_repositioning!(stdout, self.cursor_position);
        }

        if (self.y() + 1) as usize == self.lines.len() {
            self.lines.push(Line::new())
        }

        self.cursor_position.1 += 1;
        self.cursor_position.0 = 2;
        cursor_repositioning!(stdout, (2, self.y()));
        return Ok(());
    }

    pub fn left(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() == 2 && self.y() > 0 {
            self.cursor_position.1 -= 1;
            self.cursor_position.0 = self.end_current_line();
            cursor_repositioning!(stdout, self.cursor_position);
        } else if self.x() > 2 {
            self.cursor_position.0 -= 1;
            cursor_repositioning!(stdout, self.cursor_position);
        }
        return Ok(());
    }

    // pub fn right(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
    //     if self.x() == self.lines[self] && self.y() > 0 {
    //         self.cursor_position.1 -= 1;
    //         cursor_repositioning!(stdout, self.cursor_position);
    //     } else if self.x() > 2 {
    //         self.cursor_position.0 -= 1;
    //         cursor_repositioning!(stdout, self.cursor_position);
    //     }
    //     return Ok(());
    // }

    fn end_current_line(&self) -> u16 {
        return self.lines[self.actual_line as usize].line.len() as u16;
    }
}
