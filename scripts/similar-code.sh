#!/usr/bin/env bash
# Reports near-duplicate functions found by similarity-rs
# (https://github.com/mizchi/similarity).
#
#   scripts/similar-code.sh                      # every pair in src/
#   scripts/similar-code.sh src/a.rs src/b.rs    # only pairs involving these files
#
# SIMILARITY_THRESHOLD (default 0.9) sets the minimum similarity. Inside GitHub
# Actions each pair becomes a ::warning annotation. Always exits 0: the report
# informs refactoring, it does not block.
set -euo pipefail

similarity-rs src --skip-test --threshold "${SIMILARITY_THRESHOLD:-0.9}" |
  awk -v files="$*" -v gha="${GITHUB_ACTIONS:-}" '
    function path(loc) { sub(/:.*/, "", loc); return loc }
    function line(loc) { sub(/^[^:]*:/, "", loc); sub(/-.*/, "", loc); return loc }
    BEGIN { n = split(files, f, " "); for (i = 1; i <= n; i++) want[f[i]] = 1 }
    /<->/ { pair = $0; next }
    /Similarity:/ && pair != "" {
      split(pair, side, " <-> ")
      sub(/^ +/, "", side[1])
      split(side[1], a, " "); split(side[2], b, " ")
      if (n > 0 && !(path(a[1]) in want)) { split(side[2], a, " "); split(side[1], b, " ") }
      if ((n == 0 || path(a[1]) in want) && !seen[pair]++) {
        found++
        msg = sprintf("%s `%s` is %s similar to `%s` (%s)", a[2], a[3], $2, b[3], b[1])
        if (gha != "") printf "::warning file=%s,line=%s,title=Similar code::%s\n", path(a[1]), line(a[1]), msg
        else printf "%s: %s\n", a[1], msg
      }
      pair = ""
    }
    END { printf "%d similar function pair(s)\n", found }
  '
