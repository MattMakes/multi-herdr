//! Record the target triple of this build, so the harness can build `horch`
//! and the fakes for the same target as the test (T-842).

fn main() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET for a build script");
    println!("cargo:rustc-env=HORCH_E2E_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
