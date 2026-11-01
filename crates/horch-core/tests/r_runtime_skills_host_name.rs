//! `host_name` fallback order. One test only: it edits the process
//! environment, so it must not share a process with another test.

#[test]
fn horch_hostname_wins_and_an_empty_one_is_skipped() {
    std::env::set_var("HORCH_HOSTNAME", "ledger-box-7");
    assert_eq!(horch_core::runtime::machine::host_name(), "ledger-box-7");

    std::env::set_var("HORCH_HOSTNAME", "");
    let name = horch_core::runtime::machine::host_name();
    assert!(!name.is_empty());
    assert_ne!(name, "ledger-box-7");

    std::env::remove_var("HORCH_HOSTNAME");
    assert_eq!(horch_core::runtime::machine::host_name(), name);
}
