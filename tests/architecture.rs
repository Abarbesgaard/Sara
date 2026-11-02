//! Architecture rules for sara, enforced with ArchUnitRust (`archunit`).
//!
//! archunit builds a dependency graph from every file under `src/` (syntax-based:
//! `use`, `mod`, and qualified paths like `crate::commands::add::f()`), so these
//! rules see edges that a line-based `use crate::...` scan misses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use archunit::{
    Checkable, FileInfo, ResultFactory, Violation, assert_passes, pattern, project_files,
    project_layers, project_slices,
};

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

/// Largest file, in non-blank lines, before it must be split by concern.
const MAX_FILE_LINES: usize = 400;

/// Runs several rules and fails once with every violation, so one broken rule
/// does not hide the others.
fn assert_all_pass(rules: &[&dyn Checkable]) {
    let violations: Vec<Violation> = rules
        .iter()
        .flat_map(|rule| rule.check().expect("architecture check should run"))
        .collect();
    let result = ResultFactory::from_violations(&violations);
    assert!(result.passed, "{}", result.message);
}

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
        .layer("domain")
        .defined_by("src/domain/**")
        .where_layer("domain")
        .may_only_depend_on_layers(&[])
        .where_layer("infrastructure")
        .may_only_depend_on_layers(&["domain"])
        .where_layer("commands")
        .may_only_depend_on_layers(&["infrastructure", "domain"])
        .where_layer("mcp")
        .may_only_depend_on_layers(&["commands", "infrastructure", "domain"])
        .where_layer("completion")
        .may_only_depend_on_layers(&["infrastructure", "domain"])
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

// ── Infrastructure: inner dependency direction ─────────────────────────────────
//
//   util, config, git
//   db               persistence; never on memory algorithms or presentation
//   memory           memory algorithms (embedding, graph) on top of db
//   project, telemetry
//   tui              presentation; never touches persistence
#[test]
fn infrastructure_modules_depend_inwards() {
    let rule = project_layers()
        .layer("util")
        .defined_by("src/infrastructure/util/**")
        .layer("config")
        .defined_by("src/infrastructure/config.rs")
        .layer("git")
        .defined_by("src/infrastructure/git.rs")
        .layer("db")
        .defined_by("src/infrastructure/db/**")
        .layer("memory")
        .defined_by("src/infrastructure/memory/**")
        .layer("project")
        .defined_by("src/infrastructure/project.rs")
        .layer("telemetry")
        .defined_by("src/infrastructure/telemetry*.rs")
        .layer("tui")
        .defined_by("src/infrastructure/tui/**")
        .where_layer("util")
        .may_only_depend_on_layers(&[])
        .where_layer("config")
        .may_only_depend_on_layers(&[])
        .where_layer("git")
        .may_only_depend_on_layers(&[])
        .where_layer("db")
        .may_only_depend_on_layers(&["util", "config"])
        .where_layer("memory")
        .may_only_depend_on_layers(&["util", "db"])
        .where_layer("project")
        .may_only_depend_on_layers(&["config", "db"])
        .where_layer("telemetry")
        .may_only_depend_on_layers(&["config"])
        .where_layer("tui")
        .may_only_depend_on_layers(&["util"]);

    assert_passes!(rule);
}

// ── Domain ─────────────────────────────────────────────────────────────────────
//
// `src/domain` holds the entities and value objects. It is pure: no framework,
// no persistence, no I/O, and one entity per file.
#[test]
fn domain_is_framework_free() {
    let rule = project_files()
        .in_path("src/domain/**")
        .should()
        .depend_on_external_modules()
        .matching("std")
        .matching("anyhow")
        .matching("chrono")
        .matching("serde")
        .matching("uuid");

    assert_passes!(rule);
}

#[test]
fn domain_does_no_io() {
    let rule = project_files().in_path("src/domain/**").should().adhere_to(
        |f: &FileInfo| {
            [
                "std::fs",
                "std::io",
                "std::net",
                "std::process",
                "std::env",
                "print!",
                "println!",
            ]
            .iter()
            .all(|needle| !f.content.contains(needle))
        },
        "not perform I/O (fs, io, net, process, env, printing)",
    );

    assert_passes!(rule);
}

/// Each domain file defines one entity — at most one `pub struct` — and is named
/// after it (`BranchRecord` → `branch_record.rs`). A file without a struct is
/// named after its `pub enum` (`AnchorState` → `anchor_state.rs`). Enums and
/// helpers that only that entity uses may live alongside it.
#[test]
fn domain_has_one_entity_per_file() {
    fn snake_case(name: &str) -> String {
        let mut out = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        }
        out
    }

    fn pub_types<'a>(content: &'a str, keyword: &str) -> Vec<&'a str> {
        content
            .lines()
            .filter_map(|l| l.strip_prefix(keyword))
            .filter_map(|rest| rest.split(|c: char| !c.is_alphanumeric()).next())
            .collect()
    }

    let rule = project_files()
        .in_path(pattern("src/domain/**").except_with_name("mod.rs"))
        .should()
        .adhere_to(
            |f: &FileInfo| {
                let stem = f.name.trim_end_matches(".rs");
                let structs = pub_types(&f.content, "pub struct ");
                match structs.as_slice() {
                    [entity] => snake_case(entity) == stem,
                    [] => pub_types(&f.content, "pub enum ")
                        .iter()
                        .any(|e| snake_case(e) == stem),
                    _ => false,
                }
            },
            "define one entity (at most one pub struct), named after the file",
        );

    assert_passes!(rule);
}

// ── Frameworks stay at the edges ───────────────────────────────────────────────

#[test]
fn only_the_composition_root_knows_clap() {
    let rule = project_files()
        .in_path(
            pattern("src/**")
                .except("src/main.rs")
                .except("src/cli.rs")
                .except("src/dispatch.rs")
                .except("src/completion.rs"),
        )
        .should_not()
        .depend_on_external_modules()
        .matching("clap")
        .matching("clap_complete");

    assert_passes!(rule);
}

#[test]
fn migrations_framework_is_owned_by_db() {
    let rule = project_files()
        .in_path(pattern("src/**").except_in_path("src/infrastructure/db/**"))
        .should_not()
        .depend_on_external_modules()
        .matching("rusqlite_migration");

    assert_passes!(rule);
}

#[test]
fn tui_framework_is_confined_to_presentation_files() {
    let rule = project_files()
        .in_path(
            pattern("src/**")
                .except_in_path("src/infrastructure/tui/**")
                .except_in_path("src/commands/*/render**")
                .except_in_path("src/commands/*/edit.rs")
                .except_in_path("src/commands/*/interactive/**"),
        )
        .should_not()
        .depend_on_external_modules()
        .matching("ratatui")
        .matching("ratatui_textarea")
        .matching("crossterm");

    assert_passes!(rule);
}

#[test]
fn presentation_does_not_touch_persistence() {
    let command_render = project_files()
        .in_path("src/commands/*/render**")
        .should_not()
        .depend_on_files()
        .in_path("src/infrastructure/db/**");
    let command_render_sql = project_files()
        .in_path("src/commands/*/render**")
        .should_not()
        .depend_on_external_modules()
        .matching("rusqlite");
    let tui_sql = project_files()
        .in_path("src/infrastructure/tui/**")
        .should_not()
        .depend_on_external_modules()
        .matching("rusqlite");

    assert_all_pass(&[&command_render, &command_render_sql, &tui_sql]);
}

#[test]
fn infrastructure_and_mcp_never_write_to_stdout() {
    // stdout belongs to the presentation layer, and in `sara mcp` it is the
    // JSON-RPC transport: a stray print corrupts the protocol.
    let rule = project_files()
        .in_path(pattern("src/infrastructure/**").except_in_path("src/infrastructure/tui/**"))
        .should()
        .adhere_to(
            |f: &FileInfo| {
                !f.content.lines().map(str::trim).any(|t| {
                    !t.starts_with("//")
                        && !t.contains("eprint")
                        && (t.contains("println!") || t.contains("print!("))
                })
            },
            "not print to stdout",
        );
    let mcp = project_files().in_path("src/mcp/**").should().adhere_to(
        |f: &FileInfo| {
            !f.content.lines().map(str::trim).any(|t| {
                !t.starts_with("//")
                    && !t.contains("eprint")
                    && (t.contains("println!") || t.contains("print!("))
            })
        },
        "not print to stdout",
    );

    assert_all_pass(&[&rule, &mcp]);
}

// ── Slice encapsulation ───────────────────────────────────────────────────────

#[test]
fn slices_are_only_entered_through_mod_rs() {
    // `mod.rs` is a slice's public surface; its other files are private to it.
    // `shared` is the deliberate exception: it is a toolbox, not a command.
    let rules: Vec<Box<dyn Checkable>> = command_slice_names()
        .into_iter()
        .filter(|s| s != "shared")
        .filter(|s| {
            std::fs::read_dir(format!("src/commands/{s}"))
                .map(|entries| entries.count() > 1)
                .unwrap_or(false)
        })
        .map(|s| {
            Box::new(
                project_files()
                    .in_path(pattern("src/**").except_in_path(format!("src/commands/{s}/**")))
                    .should_not()
                    .depend_on_files()
                    .in_path(
                        pattern(format!("src/commands/{s}/**"))
                            .except(format!("src/commands/{s}/mod.rs")),
                    ),
            ) as Box<dyn Checkable>
        })
        .collect();

    let refs: Vec<&dyn Checkable> = rules.iter().map(|r| r.as_ref()).collect();
    assert_all_pass(&refs);
}

#[test]
fn files_stay_small_and_focused() {
    let rule = project_files().in_path("src/**").should().adhere_to(
        |f: &FileInfo| f.non_blank_line_count <= MAX_FILE_LINES,
        "contain at most 400 non-blank lines (split by concern, see ARCHITECTURE.md)",
    );

    assert_passes!(rule);
}

// ── Tests live in tests/ ─────────────────────────────────────────────────────
//
// Unit tests that need private access are still compiled as child modules, but
// their source lives under tests/unit/ and is wired in with:
//
//     #[cfg(test)]
//     #[path = "../../tests/unit/<area>/<file>.rs"]
//     mod tests;

#[test]
fn src_contains_no_test_code() {
    let rule = project_files().in_path("src/**").should().adhere_to(
        |f: &FileInfo| {
            let lines: Vec<&str> = f
                .content
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            let no_test_fns = !lines.iter().any(|l| {
                l.starts_with("#[test]")
                    || l.starts_with("#[tokio::test")
                    || l.starts_with("proptest!")
                    || l.starts_with("#[should_panic")
            });
            let cfg_test_only_wires_tests_dir = lines.iter().enumerate().all(|(i, l)| {
                !l.starts_with("#[cfg(test)]")
                    || lines
                        .get(i + 1)
                        .is_some_and(|next| next.starts_with("#[path") && next.contains("tests/"))
            });
            no_test_fns && cfg_test_only_wires_tests_dir
        },
        "contain no test code (#[cfg(test)] may only wire a module from tests/)",
    );

    assert_passes!(rule);
}

#[test]
fn src_contains_only_rust_sources() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "tests") {
                    out.push(path.clone());
                }
                walk(&path, out);
            } else if path.extension().is_none_or(|e| e != "rs") {
                out.push(path);
            }
        }
    }
    let mut stray = Vec::new();
    walk(Path::new("src"), &mut stray);

    assert!(
        stray.is_empty(),
        "Test fixtures and other non-source files belong under tests/, not src/:\n{}",
        stray
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
