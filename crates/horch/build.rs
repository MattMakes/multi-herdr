//! Bakes cargo's `TARGET` into horch as `HORCH_TARGET`, so `horch self-update`
//! picks the release archive built for this machine.

fn main() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=HORCH_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
