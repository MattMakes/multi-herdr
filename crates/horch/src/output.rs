//! Writing bulk output to stdout without panicking on a closed pipe.
//!
//! `horch sessions | head` closes the pipe early. Rust ignores SIGPIPE, so the
//! write returns `BrokenPipe` and `println!` panics with a backtrace - an alarming
//! result for an ordinary shell idiom. Reading a long ledger through `head` or
//! `less` is exactly what these commands are for, so they use this instead.

use std::io::Write;

/// Write `text` to stdout, exiting quietly if the reader has gone away.
pub fn print(text: &str) {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let result = lock.write_all(text.as_bytes()).and_then(|()| lock.flush());
    if let Err(e) = result {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            // The reader closed the pipe. That is not a failure of this command.
            std::process::exit(0);
        }
        eprintln!("horch: writing to stdout failed: {e}");
        std::process::exit(1);
    }
}

/// Write `text` followed by a newline.
pub fn println(text: &str) {
    print(&format!("{text}\n"));
}
