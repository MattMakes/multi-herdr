use horch_platform::NotSupported;

#[test]
fn not_supported_names_its_concern_and_os() {
    let text = NotSupported::new("x", "windows").to_string();
    assert!(text.contains("x"), "{text}");
    assert!(text.contains("windows"), "{text}");
}

#[test]
fn not_supported_converts_to_io_unsupported() {
    let err = std::io::Error::from(NotSupported::new("signal", "windows"));
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
}

#[test]
fn not_supported_shows_its_reason() {
    let gap = NotSupported::new("sandbox", "linux").because("bwrap is not on PATH");
    assert_eq!(gap.reason, Some("bwrap is not on PATH"));
    assert_eq!(
        gap.to_string(),
        "sandbox is not supported on linux: bwrap is not on PATH"
    );
    assert_eq!(NotSupported::new("sandbox", "linux").reason, None);
}
