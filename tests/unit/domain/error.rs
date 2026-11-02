use super::*;

#[test]
fn display_names_the_broken_rule_and_its_values() {
    let cases = [
        (
            DomainError::MemoryBodyTooLarge { len: 9, limit: 5 },
            "Memory body is 9 characters (5 char limit)",
        ),
        (
            DomainError::MemoryBodyContainsSecret { reason: "api key" },
            "Memory body may contain a secret (api key)",
        ),
        (
            DomainError::UnknownMemoryStatus("odd".into()),
            "Unknown memory status 'odd'",
        ),
        (
            DomainError::UnknownRelation("kin".into()),
            "Unknown relation 'kin'",
        ),
    ];
    for (err, want) in cases {
        assert_eq!(err.to_string(), want);
    }
}
