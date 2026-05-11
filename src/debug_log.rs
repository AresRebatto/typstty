use std::fs::OpenOptions;
use std::io::Write;

pub fn log(msg: String) {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("debug.log")
        .unwrap();

    writeln!(file, "{msg}").unwrap();
}