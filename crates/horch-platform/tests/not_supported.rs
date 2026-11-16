//! `NotSupported` is the explicit value of a gap of an OS (PL-6).

use horch_platform::NotSupported;

#[test]
fn not_supported_names_its_concern_and_os() {
    let text = NotSupported {
        concern: "x",
        os: "windows",
    }
    .to_string();
    assert!(text.contains("x"), "{text}");
    assert!(text.contains("windows"), "{text}");
}

#[test]
fn not_supported_converts_to_io_unsupported() {
    let err = std::io::Error::from(NotSupported {
        concern: "signal",
        os: "windows",
    });
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
}
