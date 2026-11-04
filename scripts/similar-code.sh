#!/usr/bin/env bash
# Near-duplicate functions found by similarity-rs
# (https://github.com/mizchi/similarity).
#
#   scripts/similar-code.sh               # report every similar pair in src/ and tests/
#   scripts/similar-code.sh origin/main   # gate: pairs where a function you changed
#                                         # since origin/main looks like another one
#
# src/ is compared without its test functions, tests/ with them, so a copied
# test is caught as well as copied code.
#
# The gate fails on a pair that is new since the base, unless
# scripts/similar-code.allow declares it intended. Pairs that already existed
# on the base are reported, not failed. SIMILARITY_THRESHOLD (default 0.87) sets
# the minimum similarity: similarity-rs 0.5 scores a renamed exact copy at
# about 88%, so a higher default lets copy-paste through. Inside GitHub Actions each pair becomes an annotation.
set -euo pipefail

base="${1:-}"
threshold="${SIMILARITY_THRESHOLD:-0.87}"
allow="$(dirname "$0")/similar-code.allow"
hunks=""
known=""
scan() {
  similarity-rs src --skip-test --threshold "$threshold"
  similarity-rs tests --threshold "$threshold"
}
if [ -n "$base" ]; then
  # "path start end" for each changed line range, from a zero-context diff.
  hunks=$(git diff -U0 --diff-filter=AM "${base}...HEAD" -- 'src/*.rs' 'tests/*.rs' | awk '
    /^\+\+\+ b\// { path = substr($0, 7); next }
    /^@@/ {
      split($3, r, ","); start = substr(r[1], 2); count = (2 in r) ? r[2] : 1
      print path, start, start + (count > 0 ? count : 1) - 1
    }')
  if [ -z "$hunks" ]; then
    echo "0 similar function pair(s): no Rust source changed since ${base}"
    exit 0
  fi
  # The pairs the base already has, so the gate fails only on new ones.
  tree=$(mktemp -d)
  trap 'git worktree remove --force "$tree" >/dev/null 2>&1 || true; rm -rf "$tree"' EXIT
  git worktree add --quiet --detach "$tree" "$(git merge-base "$base" HEAD)"
  known=$(cd "$tree" && scan)
fi

# A pair's key is "file::function file::function", sorted, so line shifts and
# side order don't matter. The awk below reads the base's pairs, then the
# allow list, then this tree's pairs.
{
  printf '%s\n' "$known" | sed 's/^/BASE /'
  if [ -f "$allow" ]; then sed -e 's/#.*//' -e '/^[[:space:]]*$/d' -e 's/^/ALLOW /' "$allow"; fi
  scan
} | HUNKS="$hunks" awk -v diffmode="${base:+1}" -v gha="${GITHUB_ACTIONS:-}" '
    function path(loc) { sub(/:.*/, "", loc); return loc }
    function first(loc) { sub(/^[^:]*:/, "", loc); sub(/-.*/, "", loc); return loc + 0 }
    function last(loc)  { sub(/^.*-/, "", loc); return loc + 0 }
    function key(x, y) { return x < y ? x " " y : y " " x }
    # Did the diff touch any line of the function at loc ("path:start-end")?
    function changed(loc,   i, h) {
      for (i = 1; i <= nh; i++) {
        split(hunk[i], h, " ")
        if (h[1] == path(loc) && h[2] <= last(loc) && h[3] >= first(loc)) return 1
      }
      return 0
    }
    function pairkey(line,   side, a, b) {
      split(line, side, " <-> "); sub(/^ +/, "", side[1])
      split(side[1], a, " "); split(side[2], b, " ")
      return key(path(a[1]) "::" a[3], path(b[1]) "::" b[3])
    }
    BEGIN { nh = split(ENVIRON["HUNKS"], hunk, "\n") }
    /^BASE / { if ($0 ~ /<->/) base[pairkey(substr($0, 6))] = 1; next }
    /^ALLOW / { allowed[key($2, $3)] = 1; next }
    /<->/ { pair = $0; next }
    /Similarity:/ && pair != "" {
      split(pair, side, " <-> ")
      sub(/^ +/, "", side[1])
      split(side[1], a, " "); split(side[2], b, " ")
      if (diffmode && !changed(a[1])) { split(side[2], a, " "); split(side[1], b, " ") }
      if ((!diffmode || changed(a[1])) && !seen[pair]++) {
        found++
        k = pairkey(pair)
        level = "warning"; why = ""
        if (diffmode && (k in allowed)) why = " (declared)"
        else if (diffmode && (k in base)) why = " (already on the base)"
        else if (diffmode) { level = "error"; fresh++ }
        msg = sprintf("%s `%s` is %s similar to `%s` (%s)%s", a[2], a[3], $2, b[3], b[1], why)
        if (gha != "") printf "::%s file=%s,line=%s,title=Similar code::%s\n", level, path(a[1]), first(a[1]), msg
        else printf "%s: %s: %s\n", a[1], level, msg
      }
      pair = ""
    }
    END {
      printf "%d similar function pair(s)\n", found
      if (fresh) {
        printf "%d new pair(s): reuse the existing function, extract a helper, or declare the pair in scripts/similar-code.allow\n", fresh
        exit 1
      }
    }
  '
