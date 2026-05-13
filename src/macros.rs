#[macro_export]
macro_rules! cursor_repositioning {
    ($stdout:expr, $pos:expr) => {{
        use crossterm::ExecutableCommand;
        $stdout.execute(crossterm::cursor::MoveTo($pos.0, $pos.1))?;
    }};
}

#[macro_export]
macro_rules! rerender_current_line {
    ($stdout:expr, $current_line:expr, $lines:expr) => {{
        use crossterm::ExecutableCommand;
        use crossterm::terminal::Clear;
        use crossterm::terminal::ClearType;
        
        let c_line = $current_line;

        $stdout.execute(crossterm::cursor::Hide)?;
        $stdout.execute(crossterm::cursor::MoveTo(0, c_line))?;
        $stdout.execute(Clear(ClearType::UntilNewLine))?; //Line cleanup
        write!($stdout, "{c_line} ")?;

        for i in $lines.lines[c_line as usize].chars() {
            write!($stdout, "{i}")?;
        }
        $stdout.execute(crossterm::cursor::Show)?;
    }};
}

#[macro_export]
macro_rules! erease_current_line {
    ($stdout:expr, $line:expr, $lines:expr) => {{
        use crossterm::ExecutableCommand;
        use crossterm::terminal::Clear;
        use crossterm::terminal::ClearType;
        $stdout.execute(crossterm::cursor::MoveTo(2, $line))?;
        $stdout.execute(Clear(ClearType::UntilNewLine))?;
    }};
}

#[macro_export]
macro_rules!  rerender_lines_from_current_position{
    ($stdout:expr,  $lines:expr) => {
	    for i in $lines.cursor_position.1..($lines.lines.len()) as u16 {
	        rerender_current_line!($stdout, i, $lines);
	    }

    };
}