use crossterm::cursor;
use crossterm::terminal::{SetSize, size};
use crossterm::{
    ExecutableCommand,
    cursor::Show,
    event::{self, Event, KeyCode, KeyModifiers},
    style::*,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

use std::io::{Cursor, Write};
use std::io::{Stdout, stdout};
mod models;
use models::*;

macro_rules! cursor_repositioning {
    ($stdout:expr, $pos:expr) => {
        $stdout.execute(cursor::MoveTo($pos.0, $pos.1))?;
    };
}

fn main() -> std::io::Result<()> {
    let mut stdout: Stdout = stdout();
    let original_size = size().unwrap();
    let mut actual_cursor_position: (u16, u16) = (2, 0);

    init_terminal(&stdout, original_size)?;
    loop {
        let event = event::read()?;
        let mut lines = lines::Lines::new();
        match event {
            Event::Key(k) => {
                //exit
                if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if k.kind == event::KeyEventKind::Press {
                    if let KeyCode::Char(c) = k.code {
                        write!(stdout, "{c}")?;
                        stdout.flush()?;
                        lines.putchar(c);
                        actual_cursor_position.0 += 1;

                        //Writing number of line
                        if !lines.is_current_line_active() {
                            cursor_repositioning!(stdout, (0, actual_cursor_position.1));
                            let n_line = actual_cursor_position.1;
                            write!(stdout, "{n_line}")?;
                            stdout.flush()?;
                            lines.active_current_line();
                            cursor_repositioning!(stdout, actual_cursor_position);
                        }
                    } else if k.code == KeyCode::Backspace && actual_cursor_position.0 > 2 {
                        //TODO implement ctrl backspace
                        actual_cursor_position.0 -= 1;
                        cursor_repositioning!(stdout, actual_cursor_position);
                        write!(stdout, " ")?;
                        stdout.flush()?;
                        cursor_repositioning!(stdout, actual_cursor_position);
                    } else if k.code == KeyCode::Enter {
                        if !lines.is_current_line_active() {
                            cursor_repositioning!(stdout, (0, actual_cursor_position.1));
                            let n_line = actual_cursor_position.1;
                            write!(stdout, "{n_line}")?;
                            stdout.flush()?;
                            lines.active_current_line();
                        }
                        actual_cursor_position.1 += 1;
                        actual_cursor_position.0 = 2;
                        cursor_repositioning!(stdout, actual_cursor_position);
                    }
                }
            }
            _ => {}
        }
    }

    cleanup_terminal(&stdout, original_size)?;

    return Ok(());
}

fn init_terminal(mut stdout: &Stdout, original_size: (u16, u16)) -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    terminal::enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;

    for i in 0..original_size.1 {
        stdout.execute(cursor::MoveTo(0, i))?;
        write!(stdout, "~")?;
    }

    stdout.execute(cursor::MoveTo(2, 0))?;

    stdout.flush()?;
    return Ok(());
}
fn cleanup_terminal(mut stdout: &Stdout, original_size: (u16, u16)) -> std::io::Result<()> {
    stdout.execute(Show)?;
    stdout.execute(LeaveAlternateScreen)?;
    stdout.execute(SetSize(original_size.0, original_size.1))?;
    terminal::disable_raw_mode()?;
    return Ok(());
}
