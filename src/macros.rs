

#[macro_export]
macro_rules! cursor_repositioning {
    ($stdout:expr, $pos:expr) => {{
    	use crossterm::ExecutableCommand;
        $stdout.execute(crossterm::cursor::MoveTo($pos.0, $pos.1))?;
    }};
}
