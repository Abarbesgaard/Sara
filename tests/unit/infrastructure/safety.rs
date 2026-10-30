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

mod properties {
    use super::super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn detect_secret_never_panics(s in "\\PC{0,200}") {
            let _ = detect_secret(&s);
        }

        #[test]
        fn aws_key_is_detected_anywhere(prefix in "[a-z ]{0,40}", key in "[A-Z0-9]{16}", suffix in "[a-z ]{0,40}") {
            let text = format!("{prefix}AKIA{key}{suffix}");
            prop_assert!(detect_secret(&text).is_some());
        }

        #[test]
        fn check_size_accepts_up_to_the_limit(n in 0usize..=SIZE_LIMIT_CHARS) {
            prop_assume!(std::env::var(SIZE_LIMIT_ENV).is_err());
            prop_assert!(check_size(&"é".repeat(n)).is_ok());
        }

        #[test]
        fn check_size_rejects_over_the_limit(extra in 1usize..500) {
            prop_assume!(std::env::var(SIZE_LIMIT_ENV).is_err());
            prop_assert!(check_size(&"é".repeat(SIZE_LIMIT_CHARS + extra)).is_err());
        }
    }
}
