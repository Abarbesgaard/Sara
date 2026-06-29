/// Architecture enforcement tests.
///
/// These tests parse source files as text and assert the five invariants
/// defined in ARCHITECTURE.md. They run with `cargo test` and produce
/// human-readable failure messages that name the offending file and line.
use std::path::{Path, PathBuf};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Return the path to every `src/commands/<name>/mod.rs` that exists.
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

/// Scan every command slice for lines containing `needle`.
/// Returns `"<file>:<line>: <content>"` for each hit.
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

// ── invariant 1 ──────────────────────────────────────────────────────────────

/// A command slice must not import another command slice.
///
/// Violation example (forbidden inside src/commands/done/mod.rs):
///   use crate::commands::list;
#[test]
fn test_no_cross_slice_dependencies() {
    let violations = scan_slices("use crate::commands::");
    assert!(
        violations.is_empty(),
        "Invariant 1 broken — cross-slice imports found:\n{}",
        violations.join("\n")
    );
}

// ── invariant 2 ──────────────────────────────────────────────────────────────

/// A command slice may only import from `crate::infrastructure::*`.
/// Importing `crate::cli` is forbidden — CLI types are wired in main.rs.
#[test]
fn test_commands_only_depend_on_infrastructure() {
    let violations = scan_slices("use crate::cli");
    assert!(
        violations.is_empty(),
        "Invariant 2 broken — commands importing from crate::cli (forbidden):\n{}",
        violations.join("\n")
    );
}

// ── invariant 3 ──────────────────────────────────────────────────────────────

/// All DB migrations must live in `src/infrastructure/db.rs` only.
/// No command slice may call `Migrations::new` or `M::up`.
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

// ── invariant 4 ──────────────────────────────────────────────────────────────

/// Every directory under `src/commands/` must contain a `mod.rs`.
/// A new command added as a bare directory with no mod.rs is caught here.
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

// ── invariant 5 ──────────────────────────────────────────────────────────────

/// `init_terminal()` must only be *defined* in `src/infrastructure/tui/mod.rs`.
/// Command slices may call it, but may not redefine it.
///
/// We scan for `fn init_terminal` (the definition signature) rather than the
/// bare string `init_terminal`, so call-sites in commands don't trigger this.
#[test]
fn test_tui_infrastructure_is_centralized() {
    let violations = scan_slices("fn init_terminal");
    assert!(
        violations.is_empty(),
        "Invariant 5 broken — init_terminal() defined outside src/infrastructure/tui/mod.rs:\n{}",
        violations.join("\n")
    );
}

// ── naming conventions ────────────────────────────────────────────────────────

/// Every directory under `src/commands/` must have a matching `pub mod <name>;`
/// in `src/commands/mod.rs`, and vice-versa.
///
/// This catches a new command added as a directory without updating mod.rs
/// (silently unreachable) or a pub mod declaration pointing at a non-existent
/// directory (compile error, but caught here first with a clear message).
#[test]
fn test_slice_dirs_match_mod_declarations() {
    let commands_dir = Path::new("src/commands");

    // Names declared in mod.rs
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

    // Names that exist as subdirectories
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

// ── query ownership ───────────────────────────────────────────────────────────

/// Raw SQL must only live in `src/infrastructure/db.rs`.
/// Command slices must not embed SQL strings directly — they call db helpers.
///
/// We scan for SQL statement keywords that would appear at the start of a
/// query string literal (`"SELECT `, `"INSERT `, etc.).
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
