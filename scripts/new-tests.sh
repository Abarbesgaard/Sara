#!/usr/bin/env bash
# Does each test a PR adds catch a bug no other test catches (IMP Rule 26)?
#
#   scripts/new-tests.sh origin/main
#
# For every test function added since the base, it breaks the code the test is
# about and runs scripts/kill-matrix.sh --test on it:
#   - a Contract test (tests/contract/<p>.rs with contract/<p>/README.md): the
#     files in the primitive's covers block;
#   - a unit test (tests/unit/…, compiled into src/ by #[path]): the src file
#     that includes it;
#   - any other test: the src files the PR changes.
# It fails when a test catches nothing, or nothing another test doesn't, and
# names the tests that share its kills. A Contract test is the spec, so for
# one those other tests are the duplicates: fold them into the Contract or
# delete them in the same PR. It also fails when the PR adds more than NEW_TESTS_MAX
# tests (default 3; each takes about 20 minutes): split the PR.
# NEW_TESTS_DRY=1 only lists each new test and the files it would break.
set -euo pipefail

base="${1:?give the base, e.g. origin/main}"
max="${NEW_TESTS_MAX:-3}"
dir="$(dirname "$0")"
gha="${GITHUB_ACTIONS:-}"

# "<fn>" for each test function in a Rust file on stdin: a fn after #[test],
# #[tokio::test] or #[impish::contract(...)].
test_fns() {
  awk '
    /^[[:space:]]*#\[(test|tokio::test|impish::contract)/ { tagged = 1; next }
    tagged && /^[[:space:]]*#\[/ { next }
    tagged && match($0, /fn [A-Za-z0-9_]+/) { print substr($0, RSTART + 3, RLENGTH - 3) }
    { tagged = 0 }
  '
}

# The module path ("commands::add::tests") of a test file under tests/unit,
# or nothing when no src file includes it.
unit_module() {
  local file="$1" inc mod parent
  inc=$(grep -rln --include='*.rs' "#\[path = \".*${file}\"\]" src | head -n 1 || true)
  if [ -n "$inc" ]; then
    mod=$(grep -A1 "#\[path = \".*${file}\"\]" "$inc" | sed -nE 's/^[[:space:]]*(pub )?mod ([A-Za-z0-9_]+);.*/\2/p' | head -n 1)
    inc="${inc#src/}"; inc="${inc%.rs}"; inc="${inc%/mod}"; inc="${inc%main}"
    inc="${inc//\//::}"
    echo "${inc:+$inc::}$mod"
    return
  fi
  parent="$(dirname "$file")/mod.rs"
  [ "$file" != "$parent" ] && [ -f "$parent" ] || return 0
  mod=$(unit_module "$parent")
  [ -z "$mod" ] || echo "$mod::$(basename "$file" .rs)"
}

# The src file that includes a unit test file, directly or through a mod.rs.
unit_src() {
  local file="$1" parent
  while :; do
    grep -rl --include='*.rs' "#\[path = \".*${file}\"\]" src | head -n 1 | grep . && return
    parent="$(dirname "$file")/mod.rs"
    [ "$file" != "$parent" ] && [ -f "$parent" ] || return 0
    file="$parent"
  done
}

changed_src=$(git diff --name-only --diff-filter=AM "${base}...HEAD" -- 'src/*.rs' | tr '\n' ' ')

# "<test id><TAB><globs>" for each added test.
added=$(git diff --name-only --diff-filter=AM "${base}...HEAD" -- 'tests/*.rs' | while read -r file; do
  comm -13 <(git show "$(git merge-base "$base" HEAD):$file" 2>/dev/null | test_fns | sort -u) \
           <(test_fns < "$file" | sort -u) | while read -r fn; do
    case "$file" in
      (tests/contract/*)
        p=$(basename "$file" .rs)
        globs="$changed_src"
        if [ -f "contract/$p/README.md" ]; then
          # shellcheck disable=SC2016 # the backticks are a literal markdown fence
          globs=$(sed -n '/^```covers$/,/^```$/p' "contract/$p/README.md" | grep '::' | sed 's/::.*//' | sort -u | tr '\n' ' ')
        fi
        printf 'contract/%s::%s\t%s\n' "$p" "$fn" "$globs" ;;
      (tests/unit/*)
        mod=$(unit_module "$file")
        [ -n "$mod" ] && printf 'sara/%s::%s\t%s\n' "$mod" "$fn" "$(unit_src "$file")" ;;
      (tests/*.rs)
        printf '%s/%s\t%s\n' "$(basename "$file" .rs)" "$fn" "$changed_src" ;;
    esac
  done
done)

if [ -z "$added" ]; then
  echo "No tests added since ${base}."
  exit 0
fi

[ -z "${NEW_TESTS_DRY:-}" ] || { printf '%s\n' "$added"; exit 0; }

status=0
fail() {
  status=1
  if [ -n "$gha" ]; then echo "::error title=New test::$1"; else echo "error: $1"; fi
}

sharers() { printf '%s\n' "$1" | sed -n 's/^Shared with: //p'; }

checked=0
while IFS=$'\t' read -r id globs; do
  if [ "$checked" -ge "$max" ]; then
    fail "$id was not checked: the PR adds more than $max tests (NEW_TESTS_MAX). Split it."
    continue
  fi
  if [ -z "${globs// /}" ]; then
    echo "Skipping $id: no code to break (it is not a Contract or unit test, and the PR changes no src file)."
    continue
  fi
  checked=$((checked + 1))
  [ -z "$gha" ] || echo "::group::$id against $globs"
  # shellcheck disable=SC2086 # globs is a list of paths
  verdict=$("$dir/kill-matrix.sh" --test "$id" $globs 2>&1 | tee /dev/stderr | tail -n 30)
  [ -z "$gha" ] || echo "::endgroup::"
  case "$verdict" in
    *"kills nothing"*) fail "$id catches no mutant in $globs: it would pass whatever that code did." ;;
    *"is a Contract test"*) fail "$id is a Contract test and every bug it catches is also caught by: $(sharers "$verdict"). They duplicate the Contract: fold them into it or delete them." ;;
    *"Every kill is shared"*) fail "$id catches nothing these tests don't already catch: $(sharers "$verdict"). Delete it, or make it catch something new." ;;
    *"Keep "*) echo "$id catches a bug no other test catches." ;;
    *) fail "$id: the kill matrix gave no verdict; see the log." ;;
  esac
done <<< "$added"
exit "$status"
