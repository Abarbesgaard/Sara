#!/usr/bin/env bash
# Tests for the scripts and workflows that make up the CI pipeline. Each case
# builds a throwaway git repo, fakes the slow tools (cargo, the kill
# matrix) and checks a script's output and exit code.
#
#   tests/pipeline/run.sh            # every case
#   tests/pipeline/run.sh kill       # cases whose name contains "kill"
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

# ── kill-matrix.sh report ──────────────────────────────────────────────────

# m1 is killed by a Contract test and a unit test, m2 by another unit test.
# nextest names a failed test again in its summary, so a kill can be listed twice.
kill_matrix_report_keeps_contract_tests() {
  repo km
  printf '#[test]\n#[impish::contract("add/A1")]\nfn spec() {}\n' > tests/contract/add.rs
  printf 'm1\tcontract/add::spec\nm1\tsara/x::dup\nm2\tsara/x::own\nm2\tsara/x::own\n' > "$work/matrix.tsv"
  printf 'contract/add::spec\nsara/x::dup\nsara/x::own\nsara/x::idle\narchitecture/layers\n' > "$work/tests.txt"
  sed '/^case /,$d' "$root/scripts/kill-matrix.sh" > "$work/km-lib.sh"
  local out rc suspects
  # shellcheck disable=SC2016 # expands in the inner shell
  out=$(KILLMX_OUT="$work/kmout" "$sh" -c 'source "$1"; report "$2" "$3" "$4"' _ \
        "$work/km-lib.sh" "$work/matrix.tsv" "$work/tests.txt" "$work/suspects.tsv" 2>&1); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "contract/add::spec   (Contract)" && expect_has "$out" "sara/x::dup   <- removable" &&
    expect_lacks "$out" "own   <- removable" && expect_has "$out" "   1    1  sara/x::own" || return
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
printf '%s\n' "$*" >> "$(dirname "$0")/cargo.args"
while [ $# -gt 0 ]; do [ "$1" = --output ] && out="$2"; shift; done
mkdir -p "$out/mutants.out/log"
m() { printf '{"scenario":{"Mutant":{"name":"%s"}},"summary":"%s","log_path":"log/%s.log"}' "$1" "$2" "$1"; }
case "$out" in
  */one) rest="$(m m1 CaughtMutant),$(m m2 CaughtMutant)" ;;
  *) rest="$(m m1 CaughtMutant),$(m m2 MissedMutant)"
     printf '        PASS [   0.010s] (  1/934) sara-tasks::behaviour mcp::t\n        FAIL [   0.020s] (  2/934) sara-tasks::bin/sara a::other\n' > "$out/mutants.out/log/m1.log" ;;
esac
printf '{"outcomes":[{"scenario":"Baseline","summary":"Success","log_path":"log/b.log"},%s]}' "$rest" > "$out/mutants.out/outcomes.json"
FAKE
  chmod +x "$work/bin/cargo"
}

# Shard a alone ties dup and own, so it would keep dup; merged with shard b,
# own covers both mutants and dup is the one that can go.
kill_matrix_merges_shards_before_judging() {
  repo kmmerge
  printf '#[test]\nfn spec() {}\n' > tests/contract/add.rs
  mkdir -p "$work/shard-a" "$work/shard-b"
  printf 'm1\tsara/x::dup\nm1\tsara/x::own\n' > "$work/shard-a/matrix.tsv"
  printf 'm2\tsara/x::own\n' > "$work/shard-b/matrix.tsv"
  printf 'sara/x::dup\nsara/x::own\nsara/x::idle\n' | tee "$work/shard-a/tests.txt" > "$work/shard-b/tests.txt"
  local out rc
  out=$(KILLMX_OUT="$work/kmmerge-out" "$sh" "$root/scripts/kill-matrix.sh" --merge "$work/shard-a" "$work/shard-b" 2>&1); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "2 caught mutant(s), killed by 2 of 3 test(s)." &&
    expect_has "$out" "sara/x::dup   <- removable" && expect_lacks "$out" "own   <- removable" && ok
}

kill_matrix_keeps_a_test_whose_last_mutant_only_it_catches() {
  repo kmlast
  printf '#[test]\nfn spec() {}\n' > tests/contract/add.rs
  cp "$root/scripts/kill-matrix.sh" scripts/
  fake_mutants
  local out rc
  out=$(PATH="$work/bin:$PATH" KILLMX_OUT="$work/kmlast-out" "$sh" scripts/kill-matrix.sh --test behaviour/mcp::t src/lib.rs 2>&1); rc=$?
  expect_exit $rc 0 "$out" && expect_has "$out" "Keep behaviour/mcp::t: it alone catches 1 mutant(s):" || return
  expect_has "$(cat "$work/bin/cargo.args")" "--test behaviour -E binary_id(*::behaviour) & test(=mcp::t)" &&
    expect_has "$(cat "$work/bin/cargo.args")" "-E not (binary_id(*::behaviour) & test(=mcp::t))" && ok
}

# Kills and passes come from nextest status lines, a unit test's binary
# <crate>::bin/sara named target sara.
kill_matrix_reads_tests_from_a_nextest_log() {
  sed '/^case /,$d' "$root/scripts/kill-matrix.sh" > "$work/km-lib.sh"
  printf '%s\n' \
    '    Starting 4 tests across 2 binaries' \
    '       START [         ] (1/4) sara-tasks::contract add::spec' \
    '        SLOW [> 60.000s] (3/4) sara-tasks::behaviour cite::slow' \
    '        PASS [   0.010s] (1/4) sara-tasks::contract add::spec' \
    $'\033[31m        FAIL\033[0m [   0.020s] (2/4) sara-tasks::bin/sara commands::x::own' \
    '     TIMEOUT [  60.000s] (3/4) sara-tasks::behaviour cite::slow' \
    '        PASS [   0.030s] (4/4) sara-tasks::bin/sara commands::x::fine' \
    '     Summary [   0.100s] 4 tests run: 2 passed, 2 failed' \
    '        FAIL [   0.020s] (2/4) sara-tasks::bin/sara commands::x::own' > "$work/nx.log"
  local failed passed
  # shellcheck disable=SC2016 # expands in the inner shell
  failed=$("$sh" -c 'source "$1"; tests_in_log "$2" FAILED' _ "$work/km-lib.sh" "$work/nx.log" | sort -u | tr '\n' ' ')
  # shellcheck disable=SC2016 # expands in the inner shell
  passed=$("$sh" -c 'source "$1"; tests_in_log "$2" ok' _ "$work/km-lib.sh" "$work/nx.log" | tr '\n' ' ')
  [ "$failed" = "behaviour/cite::slow sara/commands::x::own " ] || { bad "failed: '$failed'"; return; }
  [ "$passed" = "contract/add::spec sara/commands::x::fine " ] || { bad "passed: '$passed'"; return; }
  ok
}

cases=$(declare -F | awk '{ print $3 }' | grep -E '^(workflows|scripts|new_tests|kill_matrix)_' |
  grep -vE '^(new_tests_repo|new_tests_verdict)$')
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
