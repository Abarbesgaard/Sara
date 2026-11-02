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
