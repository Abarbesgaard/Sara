//! Holds the Contract under `contract/` to its tests; see contract/README.md.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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
    let block = readme
        .split_once("```covers\n")
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(block, _)| block)
        .ok_or("has no ```covers block")?;
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

/// `(test name, obligation tags)` for every `#[test]` in a contract test file.
fn tagged_tests(src: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut tests = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[test]" {
            continue;
        }
        let name = lines[i + 1..]
            .iter()
            .find_map(|l| l.trim().strip_prefix("fn "))
            .and_then(|l| l.split('(').next())
            .unwrap_or("?")
            .to_string();
        let tags = lines[..i]
            .iter()
            .rev()
            .take_while(|l| l.trim_start().starts_with("//") || l.trim_start().starts_with("#["))
            .filter_map(|l| l.trim().strip_prefix(TAG))
            .flat_map(|t| t.split(',').map(|s| s.trim().to_string()))
            .collect();
        tests.push((name, tags));
    }
    tests
}

fn check_primitive(
    name: &str,
    readme: &str,
    tests: &str,
    src_has: impl Fn(&str, &str) -> bool,
) -> Vec<String> {
    let mut errors = Vec::new();
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
    for (test, tags) in tagged_tests(tests) {
        if tags.is_empty() {
            errors.push(format!("{name}::{test} names no obligation"));
        }
        for tag in tags {
            let num = tag
                .strip_prefix(&format!("{name}/A"))
                .and_then(|n| n.parse::<u32>().ok());
            match num {
                Some(n) if ids.contains(&n) => *proved.entry(n).or_default() += 1,
                _ => errors.push(format!(
                    "{name}::{test} names `{tag}`, which contract/{name} doesn't declare"
                )),
            }
        }
    }
    for id in ids.iter().filter(|id| !proved.contains_key(id)) {
        errors.push(format!("{name}/A{id} has no test"));
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

#[test]
fn the_contract_holds() {
    let root = root();
    let mut errors = Vec::new();
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
        errors.extend(check_primitive(&name, &readme, &tests, defines));
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
    check_primitive("p", readme, tests, |_, f| f == "f")
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
