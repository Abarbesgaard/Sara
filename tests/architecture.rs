use std::path::{Path, PathBuf};

fn command_slice_files() -> Vec<PathBuf> {
    let commands_dir = Path::new("src/commands");
    std::fs::read_dir(commands_dir)
        .expect("src/commands/ must exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.path().join("mod.rs"))
        .filter(|p| p.exists())
        .collect()
}

fn scan_slices(needle: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for path in command_slice_files() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        for (i, line) in content.lines().enumerate() {
            if line.contains(needle) {
                hits.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    }
    hits
}

#[test]
fn test_no_cross_slice_dependencies() {
    let violations: Vec<String> = scan_slices("use crate::commands::")
        .into_iter()
        .filter(|line| !line.contains("use crate::commands::shared"))
        .collect();
    assert!(
        violations.is_empty(),
        "Invariant 1 broken — cross-slice imports found (only crate::commands::shared is allowed):\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_commands_only_depend_on_infrastructure() {
    let violations = scan_slices("use crate::cli");
    assert!(
        violations.is_empty(),
        "Invariant 2 broken — commands importing from crate::cli (forbidden):\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_db_migrations_are_centralized() {
    let mut violations = scan_slices("Migrations::new");
    violations.extend(scan_slices("M::up("));
    assert!(
        violations.is_empty(),
        "Invariant 3 broken — DB migrations found outside src/infrastructure/db.rs:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_command_slices_have_proper_structure() {
    let commands_dir = Path::new("src/commands");
    let missing: Vec<String> = std::fs::read_dir(commands_dir)
        .expect("src/commands/ must exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .filter(|e| !e.path().join("mod.rs").exists())
        .map(|e| e.path().display().to_string())
        .collect();

    assert!(
        missing.is_empty(),
        "Invariant 4 broken — command slice directories missing mod.rs:\n{}",
        missing.join("\n")
    );
}

#[test]
fn test_tui_infrastructure_is_centralized() {
    let violations = scan_slices("fn init_terminal");
    assert!(
        violations.is_empty(),
        "Invariant 5 broken — init_terminal() defined outside src/infrastructure/tui/mod.rs:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_slice_dirs_match_mod_declarations() {
    let commands_dir = Path::new("src/commands");

    let mod_rs = std::fs::read_to_string(commands_dir.join("mod.rs"))
        .expect("src/commands/mod.rs must exist");
    let declared: std::collections::BTreeSet<String> = mod_rs
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("pub mod ")
                .and_then(|s| s.strip_suffix(';'))
                .map(|name| name.to_string())
        })
        .collect();

    let on_disk: std::collections::BTreeSet<String> = std::fs::read_dir(commands_dir)
        .expect("src/commands/ must exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    let missing_decl: Vec<&String> = on_disk.difference(&declared).collect();
    let missing_dir: Vec<&String> = declared.difference(&on_disk).collect();

    assert!(
        missing_decl.is_empty() && missing_dir.is_empty(),
        "Slice/mod.rs mismatch.\n\
         Directories with no pub mod declaration: {:?}\n\
         pub mod declarations with no directory:  {:?}",
        missing_decl,
        missing_dir,
    );
}

#[test]
fn test_sql_query_ownership() {
    const SQL_KEYWORDS: &[&str] = &[
        "\"SELECT ",
        "\"INSERT ",
        "\"UPDATE ",
        "\"DELETE ",
        "\"CREATE TABLE",
        "\"DROP TABLE",
        "\"ALTER TABLE",
    ];

    let mut violations = Vec::new();
    for keyword in SQL_KEYWORDS {
        violations.extend(scan_slices(keyword));
    }

    assert!(
        violations.is_empty(),
        "Raw SQL found in command slices — queries must live in src/infrastructure/db.rs:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_acceptance_gate_never_prints_directly() {
    let src = std::fs::read_to_string("src/commands/guide/mod.rs")
        .expect("src/commands/guide/mod.rs must exist");

    let start = src
        .find("fn run_acceptance_gate")
        .expect("run_acceptance_gate must exist");

    let body: Vec<&str> = src[start..]
        .lines()
        .enumerate()
        .take_while(|(n, l)| *n == 0 || !l.starts_with('}'))
        .map(|(_, l)| l)
        .collect();

    let violations: Vec<String> = body
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim();
            !t.starts_with("//")
                && !t.contains("eprintln!")
                && (t.contains("println!") || t.contains("print!"))
        })
        .map(|(n, l)| format!("  run_acceptance_gate + {n} lines: {}", l.trim()))
        .collect();

    assert!(
        violations.is_empty(),
        "run_acceptance_gate prints to stdout directly. In MCP (`Capture`) mode \
         stdout is a JSON-RPC transport and this corrupts it. Route the message \
         through `mode.say(...)` instead:\n{}",
        violations.join("\n")
    );
}
