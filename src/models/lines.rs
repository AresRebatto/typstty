use super::super::debug_log::log;
use crate::models::line;

use super::super::{cursor_repositioning, erease_current_line, rerender_current_line};
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
            //2 cause 0 is tilde and 1 is a white space
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
        if self.is_eol() {
            //set the line number
            if self.is_current_line_empty() {
                cursor_repositioning!(stdout, (0, self.y()));

                let n_line = self.y();
                write!(stdout, "{n_line}")?;
                stdout.flush()?;

                cursor_repositioning!(stdout, self.cursor_position);
            }

            //Write the character
            self.lines[self.actual_line as usize].push_ch(c);
            write!(stdout, "{c}")?;
            stdout.flush()?;
        } else {
            self.lines[self.actual_line as usize]
                .line
                .insert((self.cursor_position.0 - 2) as usize, c);
            rerender_current_line!(stdout, self.cursor_position, self);
        }

        //FIXME not a real bug, but this here with the inserting in the
        // middle of the line envolve in buggy things
        self.cursor_position.0 += 1;
        cursor_repositioning!(stdout, self.cursor_position);
        return Ok(());
    }

    pub fn pop_char(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() != 2 {
            if self.is_eol() {
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
           
            if self.is_eof() && self.lines.len() > 1 {
                cursor_repositioning!(stdout, (0, self.y()));
                write!(stdout, "~")?;

                //When you return to the previous line, place the cursor at the end of what you had written before.
                // This saves that position.
                let x_coordinate =
                    (self.lines[(self.actual_line - 1) as usize].line.len() + 2) as u16;

                erease_current_line!(stdout, self.cursor_position, self);
                let popped = self.lines.pop().unwrap().line;
                self.lines[(self.actual_line - 1) as usize]
                    .line
                    .push_str(popped.as_str());

                self.cursor_position.1 -= 1;
                self.cursor_position.0 = x_coordinate;
                self.actual_line -= 1;
                
                rerender_current_line!(stdout, self.cursor_position, self);
                cursor_repositioning!(stdout, self.cursor_position);
            } else {

                //TODO remove in case it isn't the last line of code
            }

            //TODO erase current line
        }

        return Ok(());
    }

    pub fn newline(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        //set the line number
        if self.is_current_line_empty() {
            cursor_repositioning!(stdout, (0, self.y()));
            let n_line = self.y();
            write!(stdout, "{n_line}")?;
            stdout.flush()?;
            cursor_repositioning!(stdout, self.cursor_position);
        }

        //create new line
        if (self.y() + 1) as usize == self.lines.len() {
            self.lines.push(Line::new())
        }
        self.cursor_position.1 += 1;
        self.cursor_position.0 = 2;
        self.actual_line += 1;
        cursor_repositioning!(stdout, self.cursor_position);

        return Ok(());
    }

    pub fn left(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() == 2 && self.y() > 0 {
            self.actual_line -= 1;

            self.cursor_position.1 -= 1;
            self.cursor_position.0 = self.end_current_line() + 2;

            cursor_repositioning!(stdout, self.cursor_position);
        } else if self.x() > 2 {
            self.cursor_position.0 -= 1;
            cursor_repositioning!(stdout, self.cursor_position);
        }
        return Ok(());
    }

    pub fn right(&mut self, stdout: &mut Stdout) -> std::io::Result<()> {
        if self.x() < self.end_current_line() + 2 {
            self.cursor_position.0 += 1;
            cursor_repositioning!(stdout, self.cursor_position);
        } else if self.is_eol()
            && self.end_current_line() != 0
            && self.actual_line + 1 < self.lines.len() as u16
        {
            self.cursor_position.0 = 2;
            self.cursor_position.1 += 1;
            self.actual_line += 1;
            cursor_repositioning!(stdout, self.cursor_position);
        }
        return Ok(());
    }

    fn end_current_line(&self) -> u16 {
        return self.lines[self.actual_line as usize].line.len() as u16;
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

    fn is_eof(&self) -> bool {
        self.actual_line + 1 == self.lines.len() as u16
    }
}
