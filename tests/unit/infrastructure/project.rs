use super::*;
use std::path::{Path, PathBuf};

#[test]
fn home_dotfiles_repo_does_not_capture_subfolder() {
    let home = Path::new("/home/u");
    let dir = Path::new("/home/u/workspace");
    assert_eq!(
        project_root_for(dir, Some(home), Some(home)),
        PathBuf::from("/home/u/workspace")
    );
}

#[test]
fn real_repo_under_home_is_used_as_root() {
    let home = Path::new("/home/u");
    let repo = Path::new("/home/u/projects/myrepo");
    let dir = Path::new("/home/u/projects/myrepo/src");
    assert_eq!(
        project_root_for(dir, Some(repo), Some(home)),
        PathBuf::from("/home/u/projects/myrepo")
    );
}

#[test]
fn no_git_root_falls_back_to_dir() {
    let dir = Path::new("/home/u/workspace");
    assert_eq!(
        project_root_for(dir, None, Some(Path::new("/home/u"))),
        PathBuf::from("/home/u/workspace")
    );
}

#[test]
fn git_root_above_home_is_rejected() {
    let dir = Path::new("/home/u/workspace");
    assert_eq!(
        project_root_for(dir, Some(Path::new("/")), Some(Path::new("/home/u"))),
        PathBuf::from("/home/u/workspace")
    );
}

#[test]
fn git_root_equal_to_dir_is_used() {
    let home = Path::new("/home/u");
    let dir = Path::new("/home/u/projects/myrepo");
    assert_eq!(
        project_root_for(dir, Some(dir), Some(home)),
        PathBuf::from("/home/u/projects/myrepo")
    );
}

#[test]
fn file_link_relative_anchors_to_git_root_not_cwd() {
    let root = Path::new("/repo");
    let cwd = Path::new("/repo/components");
    assert_eq!(
        resolve_file_link("lib/server/game.ts", Some(root), cwd),
        "/repo/lib/server/game.ts"
    );
}

#[test]
fn file_link_absolute_passes_through() {
    assert_eq!(
        resolve_file_link("/x/y.ts", Some(Path::new("/repo")), Path::new("/repo/a")),
        "/x/y.ts"
    );
}

#[test]
fn file_link_outside_repo_falls_back_to_cwd() {
    assert_eq!(
        resolve_file_link("a/b.ts", None, Path::new("/tmp/work")),
        "/tmp/work/a/b.ts"
    );
}

#[test]
fn file_link_directory_prefix_slash_preserved() {
    assert_eq!(
        resolve_file_link("lib/", Some(Path::new("/repo")), Path::new("/repo/x")),
        "/repo/lib/"
    );
}

mod add_tokens {
    use super::*;
    use proptest::prelude::*;

    fn args(parts: &[&[String]]) -> Vec<String> {
        parts.iter().flat_map(|p| p.iter().cloned()).collect()
    }

    fn plus(tags: &[String]) -> Vec<String> {
        tags.iter().map(|t| format!("+{t}")).collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::Direct("tests/proptest-regressions/add_tokens.txt"),
            )),
            ..ProptestConfig::default()
        })]

        #[test]
        fn never_panics(args in prop::collection::vec("\\PC{0,12}", 0..8)) {
            let _ = parse_add_tokens(&args);
        }

        #[test]
        fn description_and_tags_round_trip(
            front in prop::collection::vec("[a-z]{1,6}", 0..3),
            words in prop::collection::vec("[a-z0-9]{1,8}", 1..6),
            back in prop::collection::vec("[a-z]{1,6}", 0..3),
        ) {
            let parsed = parse_add_tokens(&args(&[&plus(&front), &words, &plus(&back)]));
            prop_assert_eq!(parsed.description, words.join(" "));
            prop_assert_eq!(parsed.tags, args(&[&front, &back]));
        }

        #[test]
        fn markers_inside_the_description_stay_in_it(
            head in prop::collection::vec("[a-z]{1,6}", 1..3),
            marker in prop_oneof!["\\+[a-z]{1,4}", "project:[a-z]{1,4}", "pri:[hml]", "every:[a-z]{1,4}"],
            tail in prop::collection::vec("[a-z]{1,6}", 1..3),
        ) {
            let parsed = parse_add_tokens(&args(&[&head, std::slice::from_ref(&marker), &tail]));
            prop_assert_eq!(parsed.description, args(&[&head, &[marker], &tail]).join(" "));
            prop_assert!(parsed.tags.is_empty() && parsed.project.is_none());
            prop_assert!(parsed.priority.is_none() && parsed.recur.is_none());
        }

        #[test]
        fn priority_is_case_insensitive(p in "[hHmMlL]", word in "[a-z]{1,8}", lead in any::<bool>()) {
            let tok = format!("{}{p}", if lead { "PRI:" } else { "pri:" });
            let parsed = parse_add_tokens(&[tok, word.clone()]);
            prop_assert_eq!(parsed.priority, Some(p.to_uppercase()));
            prop_assert_eq!(parsed.description, word);
        }
    }
}
