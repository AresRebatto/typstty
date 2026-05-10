#[macro_export]
macro_rules! cursor_repositioning {
    ($stdout:expr, $pos:expr) => {
        $stdout.execute(cursor::MoveTo($pos.0, $pos.1))?;
    };
}
