use std::io::Write;

pub fn print(text: &str) {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let result = lock.write_all(text.as_bytes()).and_then(|()| lock.flush());
    if let Err(e) = result {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("horch: writing to stdout failed: {e}");
        std::process::exit(1);
    }
}

pub fn println(text: &str) {
    print(&format!("{text}\n"));
}
