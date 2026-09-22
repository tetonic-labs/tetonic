#[test]
fn prefixes_the_level() {
    assert_eq!(fixture_logging::format_log("INFO", "ready"), "[INFO] ready");
}

#[test]
fn preserves_message_and_level_without_normalization() {
    assert_eq!(
        fixture_logging::format_log("warn", "café:  two spaces"),
        "[warn] café:  two spaces"
    );
}

#[test]
fn handles_empty_fields() {
    assert_eq!(fixture_logging::format_log("", ""), "[] ");
    assert_eq!(fixture_logging::format_log("DEBUG", ""), "[DEBUG] ");
}
