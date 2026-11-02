use super::*;

#[test]
fn secret_kv_detected() {
    assert!(MemoryBody::detect_secret("api_key=abc123").is_some());
    assert!(MemoryBody::detect_secret("password: hunter2").is_some());
    assert!(MemoryBody::detect_secret("token=\"ghp_something\"").is_some());
}

#[test]
fn secret_aws_key_detected() {
    assert!(MemoryBody::detect_secret("AKIAIOSFODNN7EXAMPLE").is_some());
}

#[test]
fn secret_high_entropy_detected() {
    assert!(MemoryBody::detect_secret("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2").is_some());
}

#[test]
fn uuid_not_flagged_as_high_entropy() {
    assert!(MemoryBody::detect_secret("ref: 831c4d6e-8fcc-4ca5-b516-21bc8236acb0").is_none());
}

#[test]
fn clean_paragraph_passes() {
    assert!(
        MemoryBody::detect_secret(
            "Sara uses rusqlite for all DB access. The connection is created once in main."
        )
        .is_none()
    );
}

#[test]
fn memory_body_rejects_oversized_and_secret_text() {
    assert_eq!(
        MemoryBody::new("short clean text", 100).map(MemoryBody::into_string),
        Ok("short clean text".to_string())
    );
    assert_eq!(
        MemoryBody::new(&"é".repeat(11), 10),
        Err(DomainError::MemoryBodyTooLarge { len: 11, limit: 10 })
    );
    assert!(matches!(
        MemoryBody::new("api_key=secret123", 100),
        Err(DomainError::MemoryBodyContainsSecret { .. })
    ));
}

mod properties {
    use super::super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn detect_secret_never_panics(s in "\\PC{0,200}") {
            let _ = MemoryBody::detect_secret(&s);
        }

        #[test]
        fn aws_key_is_detected_anywhere(prefix in "[a-z ]{0,40}", key in "[A-Z0-9]{16}", suffix in "[a-z ]{0,40}") {
            let text = format!("{prefix}AKIA{key}{suffix}");
            prop_assert!(MemoryBody::detect_secret(&text).is_some());
        }
    }
}
