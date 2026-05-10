use crate::models::line;

use super::line::*;
use std::io::{Stdout, stdout, Cursor, Write};
use crate::cursor_repositioning;

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

        ob.lines.push(line::Line::new());

        ob
    }

    pub fn x(&self) -> u16 {
        self.cursor_position.0
    }

    pub fn y(&self) -> u16 {
        self.cursor_position.1
    }

    pub fn push_char(&mut self, c: char, stdout: &mut Stdout) -> std::io::Result<()>{
        //Is he adding a character at the end of the line?
        if (self.cursor_position.0 - 2) as usize == self.lines[self.actual_line as usize].line.len()
        {
            self.lines[self.actual_line as usize].push_ch(c);
            write!(stdout, "{c}")?;
            stdout.flush()?;
            
        } else {
            //TODO manage the character in the middle of line
        }
        
        self.cursor_position.0 += 1;
        
        return Ok(());
    }

    pub fn pop_char(&mut self) {
        if (self.x() - 2) as usize == self.lines[self.actual_line as usize].line.len() {
            self.lines[self.actual_line as usize].pop_ch();
            self.cursor_position.0 -= 1;
        } else {
            //TODO manage the character in the middle of line
        }
    }

    pub fn newline() {}

    pub fn is_current_line_active(&self) -> bool {
        self.lines[self.actual_line as usize].is_active
    }

    pub fn active_current_line(&mut self) {
        self.lines[self.actual_line as usize].is_active = true;
    }
}
