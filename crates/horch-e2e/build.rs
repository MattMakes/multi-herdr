fn main() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET for a build script");
    println!("cargo:rustc-env=HORCH_E2E_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
