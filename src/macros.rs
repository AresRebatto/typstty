

#[macro_export]
macro_rules! cursor_repositioning {
    ($stdout:expr, $pos:expr) => {{
    	use crossterm::ExecutableCommand;
        $stdout.execute(crossterm::cursor::MoveTo($pos.0, $pos.1))?;
    }};
}

#[macro_export]
macro_rules! rerender_current_line {
    ($stdout:expr, $pos:expr, $lines:expr) => {{
	   	use crossterm::{
	        cursor::{Hide, Show},
	        execute,
	    };
		use crossterm::ExecutableCommand;

	     execute!($stdout, Hide)?;
		$stdout.execute(crossterm::cursor::MoveTo(2, $pos.1))?;
	
	     for i in $lines.lines[$lines.cursor_position.1 as usize].line.chars() {
	         write!($stdout, "{i}")?;
	     }
	     execute!($stdout, Show)?;
    }};
}
