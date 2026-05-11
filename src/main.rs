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

use typstty::models::lines::*;

fn main() -> std::io::Result<()> {
    let mut stdout: Stdout = stdout();
    let original_size = size().unwrap();

    init_terminal(&stdout, original_size)?;
    let mut lines = Lines::new();
    loop {
        let event = event::read()?;

        match event {
            Event::Key(k) => {
                //exit
                if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if k.kind == event::KeyEventKind::Press {
                    if let KeyCode::Char(c) = k.code {
                        lines.push_char(c, &mut stdout)?;
                    } else if k.code == KeyCode::Backspace && lines.x() > 2 {
                        //TODO implement ctrl backspace
                        lines.pop_char(&mut stdout)?;
                    } else if k.code == KeyCode::Enter {
                        lines.newline(&mut stdout)?;
                    } else if k.code == KeyCode::Left {
                        lines.left(&mut stdout)?;
                    } else if k.code == KeyCode::Right {
                        lines.right(&mut stdout)?;
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
