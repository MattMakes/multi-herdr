use std::io::Write;

fn main() {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let code = {
        let mut io = ui_prop_tester::Io {
            out: &mut out,
            err: &mut err,
        };
        ui_prop_tester::run(
            std::env::args_os().collect(),
            &mut io,
            &ui_prop_tester::RealDrivers,
        )
    };
    let _ = out.flush();
    let _ = err.flush();
    std::process::exit(code);
}
