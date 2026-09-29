use super::*;

#[test]
fn size_check_passes_under_threshold() {
    assert!(check_size("short text").is_ok());
    assert!(check_size(&"x".repeat(SIZE_LIMIT_CHARS)).is_ok());
}

#[test]
fn size_check_fails_over_threshold() {
    assert!(check_size(&"x".repeat(SIZE_LIMIT_CHARS + 1)).is_err());
}

#[test]
fn secret_kv_detected() {
    assert!(detect_secret("api_key=abc123").is_some());
    assert!(detect_secret("password: hunter2").is_some());
    assert!(detect_secret("token=\"ghp_something\"").is_some());
}

#[test]
fn secret_aws_key_detected() {
    assert!(detect_secret("AKIAIOSFODNN7EXAMPLE").is_some());
}

#[test]
fn secret_high_entropy_detected() {
    assert!(detect_secret("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2").is_some());
}

#[test]
fn uuid_not_flagged_as_high_entropy() {
    assert!(detect_secret("ref: 831c4d6e-8fcc-4ca5-b516-21bc8236acb0").is_none());
}

#[test]
fn clean_paragraph_passes() {
    assert!(
        detect_secret(
            "Sara uses rusqlite for all DB access. The connection is created once in main."
        )
        .is_none()
    );
}

#[test]
fn check_memory_body_combines_both() {
    assert!(check_memory_body("short clean text").is_ok());
    assert!(check_memory_body(&"x".repeat(SIZE_LIMIT_CHARS + 1)).is_err());
    assert!(check_memory_body("api_key=secret123").is_err());
}
