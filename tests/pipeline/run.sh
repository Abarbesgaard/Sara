#!/usr/bin/env bash
# Tests for the scripts and workflows that make up the CI pipeline. Each case
# builds a throwaway git repo, fakes the slow tools (the kill matrix,
# similarity-rs) and checks a script's output and exit code.
#
#   tests/pipeline/run.sh            # every case
#   tests/pipeline/run.sh similar    # cases whose name contains "similar"
#
# BASH_UNDER_TEST (default bash) runs the scripts, so macOS can check them
# with its bash 3.2: BASH_UNDER_TEST=/bin/bash tests/pipeline/run.sh
set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
sh="${BASH_UNDER_TEST:-bash}"
only="${1:-}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export GITHUB_ACTIONS=''

ok() { echo "PASS  $case"; }
bad() { echo "FAIL  $case: $1"; [ -z "${2:-}" ] || printf '%s\n' "$2" | sed 's/^/      /'; return 1; }
expect_exit() { [ "$1" -eq "$2" ] || bad "exit $1, want $2" "$3"; }
expect_has() { printf '%s' "$1" | grep -qF -- "$2" || bad "output lacks: $2" "$1"; }
expect_lacks() { ! printf '%s' "$1" | grep -qF -- "$2" || bad "output has: $2" "$1"; }

# A fresh repo at $work/<name> with Sara's layout and one commit on main.
repo() {
  local d="$work/$1"
  rm -rf "$d"; mkdir -p "$d/scripts" "$d/src" "$d/tests/contract" "$d/tests/unit" "$d/contract"
  cd "$d" || exit 1
  git init -q -b main . && git config user.email t@t && git config user.name t
  echo 'pub fn a() {}' > src/lib.rs
  git add -A && git commit -qm base
}
commit() { git add -A && git commit -qm "$1"; }

# ── workflows ──────────────────────────────────────────────────────────────

# Without pipefail, `script | tee` takes tee's exit code and a failing gate
# passes; `shell: bash` turns pipefail on.
workflows_run_with_pipefail() {
  local f missing=""
  for f in "$root"/.github/workflows/*.yml; do
    awk '/^defaults:/ { d = 1; next } d && /^[^ #]/ { d = 0 } d && /^    shell: bash *$/ { found = 1 } END { exit !found }' "$f" ||
      missing="$missing $(basename "$f")"
  done
  [ -z "$missing" ] || bad "no top-level defaults.run.shell: bash in:$missing" && ok
}

scripts_pass_shellcheck() {
  [ "${SHELLCHECK:-1}" = 1 ] || { echo "PASS  $case (skipped: SHELLCHECK=0)"; return; }
  command -v shellcheck >/dev/null || { bad "shellcheck is not installed"; return; }
  local out
  out=$(shellcheck "$root"/scripts/*.sh "$root"/tests/pipeline/*.sh 2>&1) || { bad shellcheck "$out"; return; }
  ok
}

scripts_parse() {
  local f out=""
  for f in "$root"/scripts/*.sh "$root"/tests/pipeline/*.sh; do out="$out$("$sh" -n "$f" 2>&1)"; done
  [ -z "$out" ] || { bad "$sh -n" "$out"; return; }
  ok
}

# ── new-tests.sh ───────────────────────────────────────────────────────────

# A PR that adds one Contract test and one unit test; the kill matrix is a
# stub that prints $VERDICT.
new_tests_repo() {
  repo nt
  cp "$root/scripts/new-tests.sh" scripts/
  # shellcheck disable=SC2016 # $VERDICT expands when the stub runs
  printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$VERDICT"\n' > scripts/kill-matrix.sh
  chmod +x scripts/*.sh
  mkdir -p contract/add src/commands/add tests/unit/commands/add
  # shellcheck disable=SC2016 # a literal markdown fence
  printf '# add\n\n```covers\nsrc/commands/add/mod.rs::run\n```\n' > contract/add/README.md
  printf 'pub fn run() {}\n#[cfg(test)]\n#[path = "../../../tests/unit/commands/add/mod.rs"]\nmod tests;\n' > src/commands/add/mod.rs
  printf '#[test]\nfn old_unit() {}\n' > tests/unit/commands/add/mod.rs
  printf '#[test]\n#[impish::contract("add/A1")]\nfn old_contract() {}\n' > tests/contract/add.rs
  commit setup
  git checkout -q -b pr
  printf '\n#[test]\n#[impish::contract("add/A2")]\nfn new_contract() {}\n' >> tests/contract/add.rs
  printf '\n#[test]\nfn new_unit() {}\n' >> tests/unit/commands/add/mod.rs
  commit pr
}

new_tests_maps_each_test_to_the_code_it_is_about() {
  new_tests_repo
  local out
  out=$(NEW_TESTS_DRY=1 "$sh" scripts/new-tests.sh main 2>&1)
  expect_has "$out" "contract/add::new_contract	src/commands/add/mod.rs" &&
    expect_has "$out" "sara/commands::add::tests::new_unit	src/commands/add/mod.rs" &&
    expect_lacks "$out" "old_" && ok
}

# Runs new-tests.sh with the kill matrix answering $1; wants exit $2 and $3 in the output.
new_tests_verdict() {
  local out rc
  out=$(VERDICT="$1" NEW_TESTS_MAX=5 "$sh" scripts/new-tests.sh main 2>/dev/null); rc=$?
  expect_exit $rc "$2" "$out" && expect_has "$out" "$3"
}

new_tests_fails_unless_a_test_catches_something_of_its_own() {
  new_tests_repo
  new_tests_verdict "Keep x: it alone catches 1 mutant(s):" 0 "catches a bug no other test catches" &&
    new_tests_verdict "x kills nothing in y: a candidate to delete" 1 "catches no mutant" &&
    new_tests_verdict "$(printf 'Every kill is shared: x is a candidate to delete.\nShared with: sara/a sara/b')" 1 \
      "catches nothing these tests don't already catch: sara/a sara/b" &&
    new_tests_verdict "$(printf 'Every kill is shared: x is a Contract test, so the tests\nShared with: sara/dup')" 1 \
      "also caught by: sara/dup" &&
    new_tests_verdict "Keep x: a Contract test whose kills only other Contract tests share." 0 "catches a bug no other test catches" &&
    new_tests_verdict "garbage" 1 "gave no verdict" && ok
}

new_tests_fails_past_the_cap() {
  new_tests_repo
  local out rc
  out=$(VERDICT="Keep x" NEW_TESTS_MAX=1 "$sh" scripts/new-tests.sh main 2>/dev/null); rc=$?
  expect_exit $rc 1 "$out" && expect_has "$out" "Split it" && ok
}

new_tests_names_tests_in_other_targets() {
  repo ntb
  cp "$root/scripts/new-tests.sh" scripts/
  mkdir -p tests/behaviour && echo 'mod cite;' > tests/behaviour/main.rs && commit setup
  git checkout -q -b pr
  printf '#[test]\nfn quotes() {}\n' > tests/behaviour/cite.rs
  printf '#[test]\nfn layers() {}\n' > tests/architecture.rs
  echo 'pub fn b() {}' >> src/lib.rs
  commit pr
  local out
  out=$(NEW_TESTS_DRY=1 "$sh" scripts/new-tests.sh main 2>&1)
  expect_has "$out" "behaviour/cite::quotes	src/lib.rs" && expect_has "$out" "architecture/layers	src/lib.rs" && ok
}

new_tests_passes_without_new_tests() {
  repo nt0
  cp "$root/scripts/new-tests.sh" scripts/ && commit setup
  git checkout -q -b pr && echo 'pub fn b() {}' >> src/lib.rs && commit pr
  local out rc
  out=$("$sh" scripts/new-tests.sh main 2>&1); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "No tests added" && ok
}

# The gate as ci.yml runs it: piped to tee, under the workflow's shell.
new_tests_failure_survives_the_pipe_to_tee() {
  new_tests_repo
  local rc
  VERDICT=garbage "$sh" -eo pipefail -c 'scripts/new-tests.sh main 2>&1 | tee /dev/null' >/dev/null; rc=$?
  expect_exit $rc 1 "" && ok
}

# ── similar-code.sh ────────────────────────────────────────────────────────

# A similarity-rs that prints the pairs in .pairs ("locA nameA locB nameB pct"
# per line) under the directory it is asked to scan.
fake_similarity() {
  mkdir -p "$work/bin"
  cat > "$work/bin/similarity-rs" <<'EOF'
#!/usr/bin/env bash
[ -f .pairs ] || exit 0
while read -r la na lb nb pct; do
  case "$la" in "$1"/*) ;; *) continue ;; esac
  printf '  %s function %s <-> %s function %s\n  Similarity: %s\n\n' "$la" "$na" "$lb" "$nb" "$pct"
done < .pairs
EOF
  chmod +x "$work/bin/similarity-rs"
}

# main has a and b alike; the PR branch is checked out.
similar_repo() {
  repo sim
  fake_similarity
  cp "$root/scripts/similar-code.sh" scripts/
  : > scripts/similar-code.allow
  printf 'pub fn a() {}\npub fn b() {}\n' > src/lib.rs
  echo 'src/lib.rs:1-1 a src/lib.rs:2-2 b 95.00%' > .pairs
  commit setup
  git checkout -q -b pr
}

# The PR adds c, which looks like a.
add_c_like_a() {
  printf 'pub fn a() {}\npub fn b() {}\npub fn c() {}\n' > src/lib.rs
  echo 'src/lib.rs:3-3 c src/lib.rs:1-1 a 90.00%' >> .pairs
}

similar() { PATH="$work/bin:$PATH" "$sh" scripts/similar-code.sh main 2>&1; }

similar_code_fails_on_a_new_pair_only() {
  similar_repo && add_c_like_a && commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 1 "$out" && expect_has "$out" "1 new pair(s)" &&
    expect_has "$out" "error: function \`c\` is 90.00% similar to \`a\`" && expect_lacks "$out" "\`b\`" && ok
}

similar_code_passes_a_pair_already_on_the_base() {
  similar_repo
  printf 'pub fn a() {}\npub fn b() { }\n' > src/lib.rs
  commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "(already there)" && ok
}

similar_code_passes_a_declared_pair() {
  similar_repo && add_c_like_a
  echo 'src/lib.rs::a src/lib.rs::c  # intended' > scripts/similar-code.allow
  commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "(declared)" && ok
}

similar_code_leaves_tests_damp() {
  similar_repo
  printf '#[test]\nfn one() {}\n#[test]\nfn two() {}\n' > tests/contract/x.rs
  echo 'tests/contract/x.rs:4-4 two tests/contract/x.rs:2-2 one 88.50%' >> .pairs
  echo '// touch' >> src/lib.rs
  commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 0 "$out" && expect_lacks "$out" "\`two\`" && ok
}

similar_code_fails_on_a_shifted_pair() {
  similar_repo
  printf 'pub fn a() {}\npub fn b() {}\npub fn d() {}\n' > src/lib.rs
  printf 'src/lib.rs:1-1 a src/lib.rs:2-2 b 95.00%%\nsrc/lib.rs:1-1 a src/other.rs:1-1 z 91.00%%\n' > .pairs
  commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 1 "$out" && expect_has "$out" "(shifted)" && ok
}

similar_code_follows_a_renamed_file() {
  similar_repo
  git mv src/lib.rs src/moved.rs
  echo 'src/moved.rs:1-1 a src/moved.rs:2-2 b 95.00%' > .pairs
  commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "1 already there" && expect_has "$out" "0 contracted" && ok
}

similar_code_lists_a_contracted_pair() {
  similar_repo
  printf 'pub fn a() {}\n' > src/lib.rs
  : > .pairs
  commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "contracted: src/lib.rs::a src/lib.rs::b" && ok
}

similar_code_passes_without_rust_changes() {
  similar_repo
  echo docs > README.md && commit pr
  local out rc
  out=$(similar); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "no Rust source changed" && ok
}

# ── similar-backlog.sh ─────────────────────────────────────────────────────

# similar-code.sh stubbed to report f~g, g~h (one cluster) and x~y.
backlog_repo() {
  repo bl
  cp "$root/scripts/similar-backlog.sh" scripts/
  cat > scripts/similar-code.sh <<'EOF'
#!/usr/bin/env bash
echo 'src/a.rs:1-4: warning: function `f` is 90.00% similar to `g` (src/a.rs:5-9)'
echo 'src/a.rs:5-9: warning: function `g` is 91.00% similar to `h` (src/b.rs:1-4)'
echo 'src/c.rs:1-4: warning: function `x` is 95.00% similar to `y` (src/c.rs:9-12)'
echo '3 similar function pair(s)'
EOF
  chmod +x scripts/*.sh
}

similar_backlog_counts_pairs_and_clusters() {
  backlog_repo
  : > scripts/similar-code.allow   # an empty allow list once dropped every pair
  local out rc
  out=$("$sh" scripts/similar-backlog.sh 2>&1); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "3 similar function pair(s) in 2 cluster(s)" &&
    expect_has "$out" "| 3 | 2 | \`src/a.rs::f\` |" && ok
}

similar_backlog_leaves_out_declared_pairs() {
  backlog_repo
  echo 'src/c.rs::y src/c.rs::x' > scripts/similar-code.allow
  local out
  out=$("$sh" scripts/similar-backlog.sh 2>&1)
  expect_has "$out" "2 similar function pair(s) in 1 cluster(s)" && ok
}

# ── kill-matrix.sh report ──────────────────────────────────────────────────

# m1 is killed by a Contract test and a unit test, m2 by another unit test.
kill_matrix_report_keeps_contract_tests() {
  repo km
  printf '#[test]\n#[impish::contract("add/A1")]\nfn spec() {}\n' > tests/contract/add.rs
  printf 'm1\tcontract/add::spec\nm1\tsara/x::dup\nm2\tsara/x::own\n' > "$work/matrix.tsv"
  printf 'contract/add::spec\nsara/x::dup\nsara/x::own\nsara/x::idle\narchitecture/layers\n' > "$work/tests.txt"
  sed '/^case /,$d' "$root/scripts/kill-matrix.sh" > "$work/km-lib.sh"
  local out rc suspects
  # shellcheck disable=SC2016 # expands in the inner shell
  out=$(KILLMX_OUT="$work/kmout" "$sh" -c 'source "$1"; report "$2" "$3" "$4" "3/128"' _ \
        "$work/km-lib.sh" "$work/matrix.tsv" "$work/tests.txt" "$work/suspects.tsv" 2>&1); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "Shards done: 3/128." && expect_has "$out" "Partial:" &&
    expect_has "$out" "contract/add::spec   (Contract)" && expect_has "$out" "sara/x::dup   <- removable" &&
    expect_lacks "$out" "own   <- removable" || return
  suspects=$(cut -f1 "$work/suspects.tsv" | sort | tr '\n' ' ')
  [ "$suspects" = "sara/x::dup sara/x::idle " ] || { bad "suspects: '$suspects'"; return; }
  ok
}

# A fake `cargo mutants` for kill-matrix.sh --test: on its own the test kills
# m1 and m2; without it the suite kills m1 and misses m2, the last mutant.
fake_mutants() {
  mkdir -p "$work/bin"
  cat > "$work/bin/cargo" <<'FAKE'
#!/usr/bin/env bash
while [ $# -gt 0 ]; do [ "$1" = --output ] && out="$2"; shift; done
mkdir -p "$out/mutants.out/log"
m() { printf '{"scenario":{"Mutant":{"name":"%s"}},"summary":"%s","log_path":"log/%s.log"}' "$1" "$2" "$1"; }
case "$out" in
  */one) rest="$(m m1 CaughtMutant),$(m m2 CaughtMutant)" ;;
  *) rest="$(m m1 CaughtMutant),$(m m2 MissedMutant)"
     printf '     Running `target/debug/deps/behaviour-0123abcd`\ntest other ... FAILED\n' > "$out/mutants.out/log/m1.log" ;;
esac
printf '{"outcomes":[{"scenario":"Baseline","summary":"Success","log_path":"log/b.log"},%s]}' "$rest" > "$out/mutants.out/outcomes.json"
FAKE
  chmod +x "$work/bin/cargo"
}

kill_matrix_keeps_a_test_whose_last_mutant_only_it_catches() {
  repo kmlast
  printf '#[test]\nfn spec() {}\n' > tests/contract/add.rs
  cp "$root/scripts/kill-matrix.sh" scripts/
  fake_mutants
  local out rc
  out=$(PATH="$work/bin:$PATH" KILLMX_OUT="$work/kmlast-out" "$sh" scripts/kill-matrix.sh --test behaviour/mcp::t src/lib.rs 2>&1); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "Keep behaviour/mcp::t: it alone catches 1 mutant(s):" || return
  ok
}

cases=$(declare -F | awk '{ print $3 }' | grep -E '^(workflows|scripts|new_tests|similar|kill_matrix)_' |
  grep -vE '^(new_tests_repo|new_tests_verdict|similar_repo)$')
passed=0 failed=0
for case in $cases; do
  case "$case" in *"$only"*) ;; *) continue ;; esac
  out=$( ("$case") 2>&1 )
  case "$out" in
    *"PASS  $case"*) passed=$((passed + 1)); echo "PASS  $case" ;;
    *) failed=$((failed + 1)); printf '%s\n' "${out:-FAIL  $case: no verdict}" | grep -E '^(FAIL|      )' || echo "FAIL  $case" ;;
  esac
done
echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ] && [ "$passed" -gt 0 ]
