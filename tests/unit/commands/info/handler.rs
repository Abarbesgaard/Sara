use super::*;
use std::io::Write;

use crate::test_support::{env_guard, temp_dir};

#[cfg(unix)]
fn fake_editor(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("editor.sh");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    writeln!(f, "{body}").unwrap();
    drop(f);
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

#[cfg(unix)]
fn with_editor<F: FnOnce()>(script: &std::path::Path, f: F) {
    let mut env = env_guard();
    env.set("EDITOR", script).remove("VISUAL");
    f();
}

#[cfg(unix)]
#[test]
fn external_editor_returns_edited_content_when_changed() {
    let tmp = temp_dir();
    let script = fake_editor(tmp.path(), "echo 'edited text' > \"$1\"");
    with_editor(&script, || {
        let result = edit_text_via_external_editor("original text").unwrap();
        assert_eq!(result, Some("edited text\n".to_string()));
    });
}

#[cfg(unix)]
#[test]
fn external_editor_returns_none_when_unchanged() {
    let tmp = temp_dir();
    let script = fake_editor(tmp.path(), ": leave the file as-is");
    with_editor(&script, || {
        let result = edit_text_via_external_editor("same text").unwrap();
        assert_eq!(result, None);
    });
}

#[cfg(unix)]
#[test]
fn external_editor_returns_none_when_editor_exits_nonzero() {
    let tmp = temp_dir();
    let script = fake_editor(tmp.path(), "echo 'should be ignored' > \"$1\"\nexit 1");
    with_editor(&script, || {
        let result = edit_text_via_external_editor("original").unwrap();
        assert_eq!(result, None);
    });
}

#[cfg(unix)]
#[test]
fn external_editor_cleans_up_its_tempfile() {
    let tmp = temp_dir();
    let marker = tmp.path().join("marker");
    let script = fake_editor(tmp.path(), &format!("echo \"$1\" > {}", marker.display()));
    with_editor(&script, || {
        let _ = edit_text_via_external_editor("x").unwrap();
    });
    let tempfile_path = std::fs::read_to_string(&marker).unwrap().trim().to_string();
    assert!(!std::path::Path::new(&tempfile_path).exists());
}

fn link(url: &str) -> db::Link {
    db::Link {
        id: 1,
        url: url.to_string(),
        label: None,
        entry: chrono::Utc::now(),
    }
}

fn note(text: &str) -> db::Annotation {
    db::Annotation {
        id: 1,
        text: text.to_string(),
        entry: chrono::Utc::now(),
        kind: "comment".to_string(),
        author: "human".to_string(),
        target_kind: None,
        target_id: None,
        status: "open".to_string(),
        request_revision: false,
        resolved_by_run: None,
    }
}

#[test]
fn pr_url_prefers_pr_link_over_issue_link() {
    let links = vec![
        link("https://github.com/o/r/issues/7"),
        link("https://github.com/o/r/pull/42"),
    ];
    assert_eq!(
        pr_url_from(&links, &[]),
        Some("https://github.com/o/r/pull/42".to_string())
    );
}

#[test]
fn pr_url_falls_back_to_annotation_text() {
    let notes = vec![
        note("just a plain comment"),
        note("opened PR (https://github.com/o/r/pull/58), awaiting review."),
    ];
    assert_eq!(
        pr_url_from(&[], &notes),
        Some("https://github.com/o/r/pull/58".to_string())
    );
}

#[test]
fn pr_url_is_none_without_any_pr_reference() {
    let links = vec![link("https://github.com/o/r/issues/7")];
    let notes = vec![note("see https://github.com/o/r/issues/7 for context")];
    assert_eq!(pr_url_from(&links, &notes), None);
}
