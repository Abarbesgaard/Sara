#!/usr/bin/env bash
# Finds tests that catch nothing another test doesn't already catch (#296).
# Each mutant cargo-mutants makes is run against the whole suite; every test that
# fails "kills" that mutant. A test that kills nothing, or only mutants other
# tests also kill, is a candidate to delete. It only suggests: contract tests
# are spec, and a test may guard code that is never mutated (.cargo/mutants.toml).
#
#   scripts/kill-matrix.sh 'src/domain/**'          # sweep files, print the report
#   scripts/kill-matrix.sh --report                 # re-print the last sweep
#   scripts/kill-matrix.sh --test contract/undo::undo_reverts_the_last_command \
#                          'src/main.rs'            # does this one test earn its place?
#
# A full sweep is a local, overnight run (KILLMX_JOBS=4 on a laptop):
#
#   KILLMX_JOBS=4 scripts/kill-matrix.sh 'src/**'
#
# Test ids are <target>/<path>: `sara/…` for unit tests, `contract/…` for
# tests/contract. KILLMX_OUT (default /tmp/sara-killmx) holds cargo-mutants
# output; KILLMX_JOBS (default 1) sets cargo-mutants -j. Runs niced, under
# cargo-nextest: it runs every test binary in one parallel pool, where cargo
# test runs them one after another, about three times slower per mutant.
set -euo pipefail

out="${KILLMX_OUT:-/tmp/sara-killmx}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
# Tests that check structure, not behaviour: they kill nothing by design.
no_kills_expected='^architecture/'

mkdir -p "$out"

# Without GITHUB_ACTIONS, cargo-mutants adds no annotation per missed mutant:
# here a miss is one test's result, not the suite's.
mutants() { env -u GITHUB_ACTIONS nice -n 19 cargo mutants -j "${KILLMX_JOBS:-1}" --no-shuffle --test-tool nextest "$@"; }

# Sets the array `args` to `-f <glob>` for each glob (bash 3.2 has no mapfile).
globs() { args=(); for g in "$@"; do args+=(-f "$g"); done; }

# "mutant<TAB>outcome<TAB>log" for every mutant in a cargo-mutants output dir.
outcomes() {
  jq -r '.outcomes[] | select(.scenario != "Baseline")
         | [.scenario.Mutant.name, .summary, .log_path] | @tsv' "$1/mutants.out/outcomes.json"
}

# "<target>/<path>" for every test with result $2 (ok or FAILED) in a nextest
# log, from status lines like `FAIL [ 0.1s] (3/934) sara-tasks::bin/sara a::b`:
# binary `<crate>::bin/sara` is target `sara`, `<crate>::contract` is `contract`.
# A timeout or crash is a kill too; START and SLOW lines are not results.
tests_in_log() {
  awk -v want="$2" '
    { gsub(/\033\[[0-9;]*m/, "") }
    match($0, /^ *[A-Z][A-Z-]* \[[^]]*\]( \( *[0-9]+\/[0-9]+\))? +[^ ]+ +[^ ]+$/) {
      if (want == "ok" ? $1 != "PASS" : $1 !~ /^(FAIL|TIMEOUT|ABORT|LEAK-FAIL|SIG[A-Z]+)$/) next
      bin = $(NF - 1); sub(/^[^:]*::/, "", bin); sub(/^bin\//, "", bin)
      print bin "/" $NF
    }
  ' "$1"
}

# "contract/<file>::<fn>" for every test that proves an obligation. They are
# the Contract's spec (IMP Rule 20), so the report keeps them and judges the
# other tests against them.
spec_tests() {
  awk '
    FNR == 1 { file = FILENAME; sub(/^.*\//, "", file); sub(/\.rs$/, "", file); tagged = 0 }
    /#\[impish::contract\(/ { tagged = 1; next }
    tagged && match($0, /fn [A-Za-z0-9_]+/) {
      print "contract/" file "::" substr($0, RSTART + 3, RLENGTH - 3); tagged = 0
    }
  ' tests/contract/*.rs | sort -u
}

# Writes $out/matrix.tsv ("mutant<TAB>test" per kill) and $out/tests.txt.
collect() {
  local dir="$out/mutants.out" name summary log
  [ -f "$dir/outcomes.json" ] || { echo "no sweep in $out; run a sweep first" >&2; exit 2; }
  outcomes "$out" | while IFS=$'\t' read -r name summary log; do
    [ "$summary" = CaughtMutant ] || continue
    tests_in_log "$dir/$log" FAILED | M="$name" awk '{ print ENVIRON["M"] "\t" $0 }'
  done > "$out/matrix.tsv"
  tests_in_log "$dir/log/baseline.log" ok | sort -u > "$out/tests.txt"
}

# Report over a matrix and a test list. Also writes "<test><TAB>kills" for each
# suspect to $3: tests that kill nothing, and the tests that can be removed
# together (not every test without unique kills: two tests that only duplicate
# each other can't both go).
report() {
  local matrix="$1" tests="$2" suspects="$3"
  spec_tests > "$out/spec.txt"
  # Old rows for tests that no longer exist are dropped.
  awk -F'\t' 'FNR == NR { live[$0]; next } $2 in live' "$tests" "$matrix" > "$out/live.tsv"
  awk -F'\t' -v suspects="$suspects" -v quiet="$no_kills_expected" -v specfile="$out/spec.txt" '
    BEGIN { while ((getline line < specfile) > 0) spec[line] }
    FNR == NR { kills[$2] = kills[$2] SUBSEP $1; n[$2]++; by[$1]++; m[$1]; next }
    { all[$0] }
    END {
      for (t in n) {
        split(substr(kills[t], 2), ks, SUBSEP)
        for (i in ks) if (by[ks[i]] == 1) uniq[t]++
      }
      # Greedy set cover: keep the Contract tests, then the test covering most
      # uncovered mutants until all are covered. The rest can go together
      # without losing a kill.
      left = length(m)
      for (t in n) if (t in spec) {
        keep[t]; split(substr(kills[t], 2), ks, SUBSEP)
        for (i in ks) if (!(ks[i] in covered)) { covered[ks[i]]; left-- }
      }
      while (left > 0) {
        best = ""; bestn = 0
        for (t in n) if (!(t in keep)) {
          c = 0; split(substr(kills[t], 2), ks, SUBSEP)
          for (i in ks) if (!(ks[i] in covered)) c++
          if (c > bestn || (c == bestn && c > 0 && t < best)) { best = t; bestn = c }
        }
        if (bestn == 0) break
        keep[best]; split(substr(kills[best], 2), ks, SUBSEP)
        for (i in ks) if (!(ks[i] in covered)) { covered[ks[i]]; left-- }
      }
      printf "%d caught mutant(s), killed by %d of %d test(s).\n\n", length(m), length(n), length(all)
      print "Tests that kill something (kills, unique kills):"
      cmd = "sort -k3"
      for (t in n) printf "  %4d %4d  %s%s\n", n[t], uniq[t] + 0, t, (t in spec) ? "   (Contract)" : (t in keep) ? "" : "   <- removable" | cmd
      close(cmd)
      r = 0; for (t in n) if (!(t in keep)) r++
      printf "\n%d test(s) can be removed together without losing a kill (marked above).\n", r
      z = 0; for (t in all) if (!(t in n)) z++
      printf "%d test(s) killed nothing in what was swept.\n", z
      for (t in n) if (!(t in keep)) print t "\t" n[t] > suspects
      for (t in all) if (!(t in n) && t !~ quiet && !(t in spec)) print t "\t0" > suspects
      close(suspects)
    }
  ' "$out/live.tsv" "$tests"
}

sweep() {
  local args
  globs "$@"
  rm -rf "$out/mutants.out"
  # --no-fail-fast keeps nextest running every test, so each mutant's log
  # names every test that killed it, not just the first.
  mutants "${args[@]}" --cargo-test-arg=--no-fail-fast --output "$out" || true
  collect
  report "$out/matrix.tsv" "$out/tests.txt" "$out/suspects.tsv"
  echo; echo "Matrix (mutant, killing test): $out/matrix.tsv"
}

# Escapes a mutant name for an exact cargo-mutants --re match.
# shellcheck disable=SC2016 # the $ in the sed class is a literal
exact_re() { printf '^%s$' "$(printf '%s' "$1" | sed 's/[][\\.|$(){}?+*^]/\\&/g')"; }

target_args() {
  local target="${1%%/*}"
  if [ "$target" = sara ]; then echo "--bin sara"; else echo "--test $target"; fi
}

# The nextest filter for one test id: <target>/<path> is test `=<path>` in the
# binary named for the target.
test_filter() {
  local target="${1%%/*}" bin
  if [ "$target" = sara ]; then bin='*::bin/sara'; else bin="*::$target"; fi
  printf 'binary_id(%s) & test(=%s)' "$bin" "${1#*/}"
}

one_test() {
  local id="$1"; shift
  [ $# -gt 0 ] || { echo "give the file glob(s) the test covers, e.g. 'src/main.rs'" >&2; exit 2; }
  [[ "$id" == */* ]] || { echo "test id must be <target>/<path>, e.g. contract/undo::undo_reverts_the_last_command" >&2; exit 2; }
  local args re k n
  globs "$@"

  echo "1/2  Which mutants does $id kill on its own? (MISSED: this test alone lets it through)"
  rm -rf "$out/one"
  # shellcheck disable=SC2046 # target_args is two words on purpose
  # One test makes a tiny baseline, so the auto timeout would be too short.
  mutants "${args[@]}" --minimum-test-timeout 120 --output "$out/one" \
    -- $(target_args "$id") -E "$(test_filter "$id")" || true
  outcomes "$out/one" | awk -F'\t' '$2 == "CaughtMutant" { print $1 }' > "$out/one.kills"
  n=$(wc -l < "$out/one.kills" | tr -d ' ')
  spec_tests > "$out/spec.txt"
  if [ "$n" -eq 0 ]; then
    if grep -qxF "$id" "$out/spec.txt"; then
      echo "$id kills nothing in $*: its obligation proves nothing about this code."
    else
      echo "$id kills nothing in $*: a candidate to delete (or it tests other code)."
    fi
    return
  fi
  sed 's/^/     /' "$out/one.kills"

  echo "2/2  Does the rest of the suite catch those $n mutant(s) without it?"
  re=()
  while IFS= read -r k; do re+=(--re "$(exact_re "$k")"); done < "$out/one.kills"
  rm -rf "$out/rest"
  # --no-fail-fast so each log names every test that caught the mutant.
  mutants "${args[@]}" "${re[@]}" --cargo-test-arg=--no-fail-fast --output "$out/rest" \
    -- -E "not ($(test_filter "$id"))" || true
  outcomes "$out/rest" | awk -F'\t' '$2 == "MissedMutant" { print $1 }' > "$out/rest.missed"
  local name summary log
  outcomes "$out/rest" | while IFS=$'\t' read -r name summary log; do
    # An `if`, not `&&`: a missed last mutant would end the loop with status 1,
    # and pipefail would exit before the verdict.
    if [ "$summary" = CaughtMutant ]; then
      tests_in_log "$out/rest/mutants.out/$log" FAILED
    fi
  done | sort -u > "$out/rest.sharers"

  n=$(wc -l < "$out/rest.missed" | tr -d ' ')
  if [ "$n" -eq 0 ]; then
    if ! grep -qxF "$id" "$out/spec.txt"; then
      echo "Every kill is shared: $id is a candidate to delete."
      echo "Shared with: $(tr '\n' ' ' < "$out/rest.sharers")"
    elif grep -vxF -f "$out/spec.txt" "$out/rest.sharers" > "$out/rest.dups"; then
      # Other Contract tests are spec too; only the rest can duplicate it.
      echo "Every kill is shared: $id is a Contract test, so the tests that also catch these duplicate it; fold them into the Contract or delete them."
      echo "Shared with: $(tr '\n' ' ' < "$out/rest.dups")"
    else
      echo "Keep $id: a Contract test whose kills only other Contract tests share."
    fi
  else
    echo "Keep $id: it alone catches $n mutant(s):"
    sed 's/^/     /' "$out/rest.missed"
  fi
}

case "${1:-}" in
  --report) collect; report "$out/matrix.tsv" "$out/tests.txt" "$out/suspects.tsv" ;;
  --test) [ $# -ge 2 ] || { echo "--test needs a test id" >&2; exit 2; }; shift; one_test "$@" ;;
  -h | --help | "") sed -n '2,21p' "$0" | sed 's/^# \{0,1\}//' ;;
  *) sweep "$@" ;;
esac
