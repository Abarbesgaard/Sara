#!/usr/bin/env bash
# Reports near-duplicate functions found by similarity-rs
# (https://github.com/mizchi/similarity).
#
#   scripts/similar-code.sh               # every similar pair in src/
#   scripts/similar-code.sh origin/main   # only pairs where a function you changed
#                                         # since origin/main looks like another one
#
# SIMILARITY_THRESHOLD (default 0.9) sets the minimum similarity. Inside GitHub
# Actions each pair becomes a ::warning annotation. Always exits 0: the report
# informs refactoring, it does not block.
set -euo pipefail

base="${1:-}"
hunks=""
if [ -n "$base" ]; then
  # "path start end" for each changed line range in src/, from a zero-context diff.
  hunks=$(git diff -U0 --diff-filter=AM "${base}...HEAD" -- 'src/*.rs' | awk '
    /^\+\+\+ b\// { path = substr($0, 7); next }
    /^@@/ {
      split($3, r, ","); start = substr(r[1], 2); count = (2 in r) ? r[2] : 1
      print path, start, start + (count > 0 ? count : 1) - 1
    }')
  if [ -z "$hunks" ]; then
    echo "0 similar function pair(s): no Rust source changed since ${base}"
    exit 0
  fi
fi

similarity-rs src --skip-test --threshold "${SIMILARITY_THRESHOLD:-0.9}" |
  HUNKS="$hunks" awk -v diffmode="${base:+1}" -v gha="${GITHUB_ACTIONS:-}" '
    function path(loc) { sub(/:.*/, "", loc); return loc }
    function first(loc) { sub(/^[^:]*:/, "", loc); sub(/-.*/, "", loc); return loc + 0 }
    function last(loc)  { sub(/^.*-/, "", loc); return loc + 0 }
    # Did the diff touch any line of the function at loc ("path:start-end")?
    function changed(loc,   i, h) {
      for (i = 1; i <= nh; i++) {
        split(hunk[i], h, " ")
        if (h[1] == path(loc) && h[2] <= last(loc) && h[3] >= first(loc)) return 1
      }
      return 0
    }
    BEGIN { nh = split(ENVIRON["HUNKS"], hunk, "\n") }
    /<->/ { pair = $0; next }
    /Similarity:/ && pair != "" {
      split(pair, side, " <-> ")
      sub(/^ +/, "", side[1])
      split(side[1], a, " "); split(side[2], b, " ")
      if (diffmode && !changed(a[1])) { split(side[2], a, " "); split(side[1], b, " ") }
      if ((!diffmode || changed(a[1])) && !seen[pair]++) {
        found++
        msg = sprintf("%s `%s` is %s similar to `%s` (%s)", a[2], a[3], $2, b[3], b[1])
        if (gha != "") printf "::warning file=%s,line=%s,title=Similar code::%s\n", path(a[1]), first(a[1]), msg
        else printf "%s: %s\n", a[1], msg
      }
      pair = ""
    }
    END { printf "%d similar function pair(s)\n", found }
  '
