//! Architecture rules for sara, enforced with ArchUnitRust (`archunit`).
//!
//! archunit builds a dependency graph from every file under `src/` (syntax-based:
//! `use`, `mod`, and qualified paths like `crate::commands::add::f()`), so these
//! rules see edges that a line-based `use crate::...` scan misses.

use std::collections::BTreeSet;
use std::path::Path;

use archunit::{FileInfo, assert_passes, pattern, project_files, project_layers, project_slices};

/// A command slice is one directory under `src/commands/`; `src/commands/mod.rs`
/// is the registry and belongs to no slice.
const COMMAND_SLICE_REGEX: &str = r"^src/commands/([^/]+)/.*$";

/// Cross-slice dependencies that existed when the rule was introduced. Only
/// `commands::shared` is a legitimate cross-slice target; every edge here is debt
/// to be moved into `shared` (or infrastructure). The list is a ratchet: new edges
/// fail `command_slices_only_share_through_shared`, and removed edges must be
/// deleted here or `known_cross_slice_debt_is_still_real` fails.
const KNOWN_CROSS_SLICE_DEBT: &[(&str, &str)] = &[
    ("add", "begin"),
    ("add", "guide"),
    ("agent", "annotate"),
    ("agent", "diagnose_memories"),
    ("agent", "done"),
    ("agent", "guide"),
    ("agent", "prune_memories"),
    ("agent", "reflect"),
    ("begin", "add"),
    ("begin", "guide"),
    ("board", "info"),
    ("doctor", "diagnose_memories"),
    ("follow", "info"),
    ("info", "dream"),
    ("projects", "board"),
];

fn command_slice_names() -> BTreeSet<String> {
    std::fs::read_dir("src/commands")
        .expect("src/commands/ must exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn layers_depend_inwards() {
    let rule = project_layers()
        .layer("main")
        .defined_by("src/main.rs")
        .layer("dispatch")
        .defined_by("src/dispatch.rs")
        .layer("cli")
        .defined_by("src/cli.rs")
        .layer("completion")
        .defined_by("src/completion.rs")
        .layer("mcp")
        .defined_by("src/mcp/**")
        .layer("commands")
        .defined_by("src/commands/**")
        .layer("infrastructure")
        .defined_by("src/infrastructure/**")
        .where_layer("infrastructure")
        .may_only_depend_on_layers(&[])
        .where_layer("commands")
        .may_only_depend_on_layers(&["infrastructure"])
        .where_layer("mcp")
        .may_only_depend_on_layers(&["commands", "infrastructure"])
        .where_layer("completion")
        .may_only_depend_on_layers(&["infrastructure"])
        .where_layer("cli")
        .may_only_depend_on_layers(&["completion"]);

    assert_passes!(rule);
}

#[test]
fn command_slices_only_share_through_shared() {
    let slices = command_slice_names();
    let mut diagram = String::from("@startuml\n");
    for slice in &slices {
        diagram.push_str(&format!("component [{slice}]\n"));
        if slice != "shared" {
            diagram.push_str(&format!("[{slice}] --> [shared]\n"));
        }
    }
    for (from, to) in KNOWN_CROSS_SLICE_DEBT {
        diagram.push_str(&format!("[{from}] --> [{to}]\n"));
    }
    diagram.push_str("@enduml\n");

    let rule = project_slices()
        .defined_by_regex(COMMAND_SLICE_REGEX)
        .should()
        .ignoring_external_slices()
        .adhere_to_diagram(diagram);

    assert_passes!(rule);
}

#[test]
fn known_cross_slice_debt_is_still_real() {
    let actual = project_slices()
        .defined_by_regex(COMMAND_SLICE_REGEX)
        .to_plantuml()
        .expect("command slice graph should build");

    let stale: Vec<String> = KNOWN_CROSS_SLICE_DEBT
        .iter()
        .filter(|(from, to)| !actual.contains(&format!("[{from}] --> [{to}]")))
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect();

    assert!(
        stale.is_empty(),
        "Cross-slice debt paid off — remove these from KNOWN_CROSS_SLICE_DEBT:\n{}",
        stale.join("\n")
    );
}

#[test]
fn only_mcp_uses_the_async_mcp_stack() {
    let rule = project_files()
        .in_path(pattern("src/**").except_in_path("src/mcp/**"))
        .should_not()
        .depend_on_external_modules()
        .matching("rmcp")
        .matching("tokio")
        .matching("schemars");

    assert_passes!(rule);
}

#[test]
fn only_telemetry_uses_network_and_libc() {
    let rule = project_files()
        .in_path(pattern("src/**").except_in_path("src/infrastructure/telemetry.rs"))
        .should_not()
        .depend_on_external_modules()
        .matching("ureq")
        .matching("libc");

    assert_passes!(rule);
}

#[test]
fn db_migrations_are_centralized() {
    let rule = project_files()
        .in_path("src/commands/**")
        .should()
        .adhere_to(
            |f: &FileInfo| !f.content.contains("Migrations::new") && !f.content.contains("M::up("),
            "not define DB migrations (they live in src/infrastructure/db)",
        );

    assert_passes!(rule);
}

#[test]
fn sql_is_owned_by_infrastructure() {
    const SQL_KEYWORDS: &[&str] = &[
        "\"SELECT ",
        "\"INSERT ",
        "\"UPDATE ",
        "\"DELETE ",
        "\"CREATE TABLE",
        "\"DROP TABLE",
        "\"ALTER TABLE",
    ];
    let rule = project_files()
        .in_path("src/commands/**")
        .should()
        .adhere_to(
            |f: &FileInfo| !SQL_KEYWORDS.iter().any(|k| f.content.contains(k)),
            "not contain raw SQL (queries live in src/infrastructure/db)",
        );

    assert_passes!(rule);
}

#[test]
fn tui_infrastructure_is_centralized() {
    let rule = project_files()
        .in_path("src/commands/**")
        .should()
        .adhere_to(
            |f: &FileInfo| !f.content.contains("fn init_terminal"),
            "not define init_terminal() (it lives in src/infrastructure/tui)",
        );

    assert_passes!(rule);
}

#[test]
fn acceptance_gate_never_prints_directly() {
    let gate = project_files().in_file("src/commands/guide/gate.rs");

    let defines_gate = gate.clone().should().adhere_to(
        |f: &FileInfo| f.content.contains("fn run_acceptance_gate"),
        "define run_acceptance_gate",
    );
    assert_passes!(defines_gate);

    // In MCP (`Capture`) mode stdout is the JSON-RPC transport; printing corrupts
    // it. Route messages through `mode.say(...)` instead.
    let no_stdout = gate.should().adhere_to(
        |f: &FileInfo| {
            !f.content.lines().map(str::trim).any(|t| {
                !t.starts_with("//")
                    && !t.contains("eprintln!")
                    && (t.contains("println!") || t.contains("print!"))
            })
        },
        "not print to stdout (use mode.say(...))",
    );
    assert_passes!(no_stdout);
}

#[test]
fn command_slices_have_proper_structure() {
    let missing: Vec<String> = command_slice_names()
        .into_iter()
        .filter(|s| !Path::new("src/commands").join(s).join("mod.rs").exists())
        .collect();

    assert!(
        missing.is_empty(),
        "Command slice directories missing mod.rs:\n{}",
        missing.join("\n")
    );
}

#[test]
fn slice_dirs_match_mod_declarations() {
    let mod_rs =
        std::fs::read_to_string("src/commands/mod.rs").expect("src/commands/mod.rs must exist");
    let declared: BTreeSet<String> = mod_rs
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub mod ")?
                .strip_suffix(';')
                .map(str::to_string)
        })
        .collect();
    let on_disk = command_slice_names();

    let missing_decl: Vec<&String> = on_disk.difference(&declared).collect();
    let missing_dir: Vec<&String> = declared.difference(&on_disk).collect();

    assert!(
        missing_decl.is_empty() && missing_dir.is_empty(),
        "Slice/mod.rs mismatch.\n\
         Directories with no pub mod declaration: {missing_decl:?}\n\
         pub mod declarations with no directory:  {missing_dir:?}",
    );
}
