//! Holds the Contract under `contract/` to its tests; see contract/README.md.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::harness::Sara;

const TAG: &str = "// Contract:";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn obligations(readme: &str) -> Result<BTreeSet<u32>, String> {
    let mut ids = BTreeSet::new();
    for (n, line) in readme.lines().enumerate() {
        let Some(rest) = line.strip_prefix("- **A") else {
            continue;
        };
        let num = rest
            .split_once("** ")
            .and_then(|(num, text)| (!text.trim().is_empty()).then_some(num))
            .and_then(|num| num.parse::<u32>().ok())
            .ok_or_else(|| format!("line {}: malformed obligation `{line}`", n + 1))?;
        if !ids.insert(num) {
            return Err(format!("line {}: A{num} is declared twice", n + 1));
        }
    }
    if ids.is_empty() {
        return Err("declares no obligations".into());
    }
    Ok(ids)
}

fn covers(readme: &str) -> Result<Vec<(String, String)>, String> {
    let block = block(readme, "covers").ok_or("has no ```covers block")?;
    block
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.trim()
                .split_once("::")
                .map(|(f, func)| (f.to_string(), func.to_string()))
                .ok_or_else(|| format!("covers line `{l}` is not <file>::<function>"))
        })
        .collect()
}

fn block<'a>(readme: &'a str, kind: &str) -> Option<&'a str> {
    readme
        .split_once(&format!("```{kind}\n"))
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(block, _)| block)
}

/// The CLI commands and MCP tools a primitive's obligations are about.
fn surface(readme: &str) -> Result<Vec<String>, String> {
    let Some(block) = block(readme, "surface") else {
        return Ok(Vec::new());
    };
    block
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| match l.split_once(' ') {
            Some(("cli" | "mcp", name)) if !name.contains(' ') => Ok(l.to_string()),
            _ => Err(format!(
                "surface line `{l}` is not `cli <command>` or `mcp <tool>`"
            )),
        })
        .collect()
}

struct TaggedTest {
    name: String,
    tags: Vec<String>,
    ignored: bool,
}

fn is_ignore(line: &&str) -> bool {
    line.trim_start().starts_with("#[ignore")
}

/// Every `#[test]` in a contract test file, with its obligation tags.
fn tagged_tests(src: &str) -> Vec<TaggedTest> {
    let lines: Vec<&str> = src.lines().collect();
    let mut tests = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[test]" {
            continue;
        }
        let after: Vec<&str> = lines[i + 1..]
            .iter()
            .take_while(|l| !l.trim_start().starts_with("fn "))
            .copied()
            .collect();
        let name = lines[i + 1 + after.len()..]
            .first()
            .and_then(|l| l.trim().strip_prefix("fn "))
            .and_then(|l| l.split('(').next())
            .unwrap_or("?")
            .to_string();
        let before: Vec<&str> = lines[..i]
            .iter()
            .rev()
            .take_while(|l| l.trim_start().starts_with("//") || l.trim_start().starts_with("#["))
            .copied()
            .collect();
        let tags = before
            .iter()
            .filter_map(|l| l.trim().strip_prefix(TAG))
            .flat_map(|t| t.split(',').map(|s| s.trim().to_string()))
            .collect();
        let ignored = before.iter().chain(&after).any(is_ignore);
        tests.push(TaggedTest {
            name,
            tags,
            ignored,
        });
    }
    tests
}

fn check_primitive(
    name: &str,
    readme: &str,
    tests: &str,
    src_has: impl Fn(&str, &str) -> bool,
    known: &BTreeSet<String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    match surface(readme) {
        Ok(lines) => {
            for line in lines.iter().filter(|l| !known.contains(*l)) {
                errors.push(format!(
                    "contract/{name}: surface `{line}` is no sara command or tool"
                ));
            }
        }
        Err(e) => errors.push(format!("contract/{name}: {e}")),
    }
    let ids = match obligations(readme) {
        Ok(ids) => ids,
        Err(e) => return vec![format!("contract/{name}: {e}")],
    };
    match covers(readme) {
        Ok(lines) => {
            for (file, func) in lines {
                if !src_has(&file, &func) {
                    errors.push(format!(
                        "contract/{name}: covers `{file}::{func}`, which doesn't exist"
                    ));
                }
            }
        }
        Err(e) => errors.push(format!("contract/{name}: {e}")),
    }
    let mut proved: BTreeMap<u32, usize> = BTreeMap::new();
    let mut ignored: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for t in tagged_tests(tests) {
        let test = &t.name;
        if t.tags.is_empty() {
            errors.push(format!("{name}::{test} names no obligation"));
        }
        for tag in &t.tags {
            let num = tag
                .strip_prefix(&format!("{name}/A"))
                .and_then(|n| n.parse::<u32>().ok());
            match num {
                Some(n) if ids.contains(&n) && t.ignored => {
                    ignored
                        .entry(n)
                        .or_default()
                        .push(format!("{name}::{test}"));
                }
                Some(n) if ids.contains(&n) => *proved.entry(n).or_default() += 1,
                _ => errors.push(format!(
                    "{name}::{test} names `{tag}`, which contract/{name} doesn't declare"
                )),
            }
        }
    }
    for id in ids.iter().filter(|id| !proved.contains_key(id)) {
        match ignored.get(id) {
            Some(tests) => errors.push(format!(
                "{name}/A{id} has no test ({} is ignored)",
                tests.join(", ")
            )),
            None => errors.push(format!("{name}/A{id} has no test")),
        }
    }
    errors
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn defines(file: &str, func: &str) -> bool {
    fs::read_to_string(root().join(file)).is_ok_and(|src| {
        src.contains(&format!("fn {func}(")) || src.contains(&format!("fn {func}<"))
    })
}

/// Every CLI command and MCP tool, as `cli <command>` and `mcp <tool>`.
fn real_surface() -> BTreeSet<String> {
    let s = Sara::new();
    let help = s.run(&["--help"]);
    let commands = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|c| *c != "help")
        .map(|c| format!("cli {c}"));
    let tools = s.mcp().tools().into_iter().map(|t| format!("mcp {t}"));
    commands.chain(tools).collect()
}

fn coverage(primitives: &BTreeMap<String, (usize, Vec<String>)>, real: &BTreeSet<String>) -> Value {
    let covered: BTreeSet<&String> = primitives.values().flat_map(|(_, s)| s).collect();
    let uncovered: Vec<&String> = real.iter().filter(|s| !covered.contains(s)).collect();
    let percent = (covered.len() as f64 * 1000.0 / real.len() as f64).round() / 10.0;
    json!({
        "obligations": primitives.values().map(|(n, _)| n).sum::<usize>(),
        "primitives": primitives
            .iter()
            .map(|(name, (n, surface))| (name.clone(), json!({"obligations": n, "surface": surface})))
            .collect::<serde_json::Map<_, _>>(),
        "surface": {"covered": covered.len(), "total": real.len(), "percent": percent},
        "uncovered": uncovered,
    })
}

#[test]
fn the_contract_holds() {
    let root = root();
    let real = real_surface();
    let mut errors = Vec::new();
    let mut primitives = BTreeMap::new();
    let mut primitive_files = BTreeSet::new();
    let mut dirs: Vec<_> = fs::read_dir(root.join("contract"))
        .expect("contract/ exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let tests_path = root.join("tests/contract").join(format!("{name}.rs"));
        primitive_files.insert(tests_path.clone());
        let Ok(readme) = fs::read_to_string(dir.join("README.md")) else {
            errors.push(format!("contract/{name} has no README.md"));
            continue;
        };
        let Ok(tests) = fs::read_to_string(&tests_path) else {
            errors.push(format!("contract/{name} has no tests/contract/{name}.rs"));
            continue;
        };
        errors.extend(check_primitive(&name, &readme, &tests, defines, &real));
        let count = obligations(&readme).map_or(0, |ids| ids.len());
        let mut lines = surface(&readme).unwrap_or_default();
        lines.retain(|l| real.contains(l));
        lines.sort();
        primitives.insert(name, (count, lines));
    }

    let path = root.join("contract/coverage.json");
    let fresh = serde_json::to_string_pretty(&coverage(&primitives, &real)).unwrap() + "\n";
    if std::env::var_os("SARA_UPDATE_CONTRACT").is_some() {
        fs::write(&path, &fresh).expect("write contract/coverage.json");
    } else if fs::read_to_string(&path).ok().as_deref() != Some(fresh.as_str()) {
        errors.push(
            "contract/coverage.json is stale; refresh it with \
             `SARA_UPDATE_CONTRACT=1 cargo test --test contract obligations`"
                .into(),
        );
    }

    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    rust_files(&root.join("tests"), &mut files);
    let this = root.join("tests/contract/obligations.rs");
    for file in files
        .iter()
        .filter(|f| **f != this && !primitive_files.contains(*f))
    {
        if fs::read_to_string(file).is_ok_and(|s| s.lines().any(|l| l.trim().starts_with(TAG))) {
            errors.push(format!(
                "{} carries a Contract tag outside a primitive's test file",
                file.display()
            ));
        }
    }

    assert!(
        errors.is_empty(),
        "the Contract doesn't hold:\n  {}",
        errors.join("\n  ")
    );
}

const README: &str = "- **A1** One.\n- **A2** Two.\n\n```covers\nsrc/x.rs::f\n```\n";

fn check(readme: &str, tests: &str) -> Vec<String> {
    let known = BTreeSet::from(["cli done".to_string(), "mcp done".to_string()]);
    check_primitive("p", readme, tests, |_, f| f == "f", &known)
}

#[test]
fn a_fully_proved_primitive_holds() {
    let tests = "// Contract: p/A1, p/A2\n#[test]\nfn t() {}\n";
    assert_eq!(check(README, tests), Vec::<String>::new());
}

#[test]
fn an_unproved_obligation_fails() {
    let tests = "// Contract: p/A1\n#[test]\nfn t() {}\n";
    assert_eq!(check(README, tests), ["p/A2 has no test"]);
}

#[test]
fn an_ignored_test_proves_nothing() {
    let after = "// Contract: p/A1\n#[test]\nfn t() {}\n\n// Contract: p/A2\n#[test]\n#[ignore]\nfn u() {}\n";
    let before = after.replace("#[test]\n#[ignore]", "#[ignore = \"slow\"]\n#[test]");
    for tests in [after, before.as_str()] {
        assert_eq!(check(README, tests), ["p/A2 has no test (p::u is ignored)"]);
    }
}

#[test]
fn an_untagged_test_fails() {
    let tests = "// Contract: p/A1, p/A2\n#[test]\nfn t() {}\n\n#[test]\nfn u() {}\n";
    assert_eq!(check(README, tests), ["p::u names no obligation"]);
}

#[test]
fn a_tag_for_an_unknown_obligation_fails() {
    let tests = "// Contract: p/A1, p/A2, q/A1, p/A9\n#[test]\nfn t() {}\n";
    let errors = check(README, tests);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(
        errors[0].contains("`q/A1`") && errors[1].contains("`p/A9`"),
        "{errors:?}"
    );
}

#[test]
fn a_duplicate_or_malformed_obligation_fails() {
    let tests = "// Contract: p/A1\n#[test]\nfn t() {}\n";
    assert!(check("- **A1** a\n- **A1** b\n", tests)[0].contains("declared twice"));
    assert!(check("- **A1**\n", tests)[0].contains("malformed"));
    assert!(check("no obligations here\n", tests)[0].contains("no obligations"));
}

#[test]
fn a_covers_line_for_missing_code_fails() {
    let readme = README.replace("::f", "::gone");
    let tests = "// Contract: p/A1, p/A2\n#[test]\nfn t() {}\n";
    assert_eq!(
        check(&readme, tests),
        ["contract/p: covers `src/x.rs::gone`, which doesn't exist"]
    );
}

#[test]
fn a_surface_line_for_a_missing_command_fails() {
    let tests = "// Contract: p/A1, p/A2\n#[test]\nfn t() {}\n";
    let ok = format!("{README}\n```surface\ncli done\nmcp done\n```\n");
    assert_eq!(check(&ok, tests), Vec::<String>::new());
    let gone = format!("{README}\n```surface\ncli gone\n```\n");
    assert_eq!(
        check(&gone, tests),
        ["contract/p: surface `cli gone` is no sara command or tool"]
    );
    let bad = format!("{README}\n```surface\ndone\n```\n");
    assert!(check(&bad, tests)[0].contains("is not `cli <command>`"));
}

#[test]
fn coverage_counts_each_command_once() {
    let real = BTreeSet::from(["cli a", "cli b", "mcp a", "mcp b"].map(String::from));
    let primitives = BTreeMap::from([
        (
            "p".to_string(),
            (2, vec!["cli a".to_string(), "mcp a".to_string()]),
        ),
        ("q".to_string(), (1, vec!["cli a".to_string()])),
    ]);
    let v = coverage(&primitives, &real);
    assert_eq!(v["obligations"], 3);
    assert_eq!(
        v["surface"],
        json!({"covered": 2, "total": 4, "percent": 50.0})
    );
    assert_eq!(v["uncovered"], json!(["cli b", "mcp b"]));
}
