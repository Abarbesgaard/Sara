#!/usr/bin/env bash
# Near-duplicate functions in src/, found by similarity-rs
# (https://github.com/mizchi/similarity). This is the exhale half of IMP's
# "breathe in, breathe out": a PR folds what it learned into what already
# exists instead of leaving a copy behind.
#
#   scripts/similar-code.sh               # report every similar pair in src/
#   scripts/similar-code.sh origin/main   # gate: label each pair against origin/main
#
# Only src/ is compared, without its test functions: tests stay DAMP, not DRY.
#
# The gate compares this tree with the merge base and labels each pair:
#   introduced     the PR touched a side and the pair didn't match at the base  (fails)
#   shifted        neither side was touched, yet the pair is new                 (fails)
#   already there  the pair matched at the base too                              (warns)
#   declared       scripts/similar-code.allow keeps the pair on purpose         (warns)
# and lists the pairs the PR contracted: on the base, gone now. File renames
# are followed, so moving code is not a new pair. Untouched pairs already on
# the base are counted, not listed; scripts/similar-backlog.sh lists them.
# SIMILARITY_THRESHOLD (default 0.87) sets the minimum similarity: similarity-rs
# 0.5 scores a renamed exact copy at about 88%, so a higher default lets
# copy-paste through. Inside GitHub Actions each pair becomes an annotation.
set -euo pipefail

base="${1:-}"
threshold="${SIMILARITY_THRESHOLD:-0.87}"
allow="$(dirname "$0")/similar-code.allow"
hunks=""
renames=""
known=""
scan() { similarity-rs src --skip-test --threshold "$threshold"; }
if [ -n "$base" ]; then
  # "path start end" for each changed line range, from a zero-context diff.
  hunks=$(git diff -U0 --diff-filter=AMR "${base}...HEAD" -- 'src/*.rs' | awk '
    /^\+\+\+ b\// { path = substr($0, 7); next }
    /^@@/ {
      split($3, r, ","); start = substr(r[1], 2); count = (2 in r) ? r[2] : 1
      print path, start, start + (count > 0 ? count : 1) - 1
    }')
  # "old new" for each renamed file, so the base's pairs follow their code.
  renames=$(git diff -M --name-status --diff-filter=R "${base}...HEAD" -- 'src/*.rs' | awk -F'\t' '{ print $2, $3 }')
  if [ -z "$hunks" ] && [ -z "$renames" ]; then
    echo "0 similar function pair(s): no Rust source changed since ${base}"
    exit 0
  fi
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
} | HUNKS="$hunks" RENAMES="$renames" awk -v diffmode="${base:+1}" -v gha="${GITHUB_ACTIONS:-}" '
    function path(loc) { sub(/:.*/, "", loc); return loc }
    function first(loc) { sub(/^[^:]*:/, "", loc); sub(/-.*/, "", loc); return loc + 0 }
    function last(loc)  { sub(/^.*-/, "", loc); return loc + 0 }
    function key(x, y) { return x < y ? x " " y : y " " x }
    function moved(p) { return (p in renamed) ? renamed[p] : p }
    # Did the diff touch any line of the function at loc ("path:start-end")?
    function changed(loc,   i, h) {
      for (i = 1; i <= nh; i++) {
        split(hunk[i], h, " ")
        if (h[1] == path(loc) && h[2] <= last(loc) && h[3] >= first(loc)) return 1
      }
      return 0
    }
    function pairkey(line, follow,   side, a, b, pa, pb) {
      split(line, side, " <-> "); sub(/^ +/, "", side[1])
      split(side[1], a, " "); split(side[2], b, " ")
      pa = path(a[1]); pb = path(b[1])
      if (follow) { pa = moved(pa); pb = moved(pb) }
      return key(pa "::" a[3], pb "::" b[3])
    }
    function report(level, a, b, pct, why,   msg) {
      msg = sprintf("%s `%s` is %s similar to `%s` (%s)%s", a[2], a[3], pct, b[3], b[1], why)
      if (gha != "") printf "::%s file=%s,line=%s,title=Similar code::%s\n", level, path(a[1]), first(a[1]), msg
      else printf "%s: %s: %s\n", a[1], level, msg
    }
    BEGIN {
      nh = split(ENVIRON["HUNKS"], hunk, "\n")
      nr = split(ENVIRON["RENAMES"], ren, "\n")
      for (i = 1; i <= nr; i++) if (split(ren[i], r, " ") == 2) renamed[r[1]] = r[2]
    }
    # A pair line starts with a location; "Classes: A <-> B" lines are not pairs.
    /^BASE / { if ($0 ~ /^BASE +[^ ]+:[0-9]+-[0-9]+ .*<->/) { k = pairkey(substr($0, 6), 1); base[k] = 1; name[k] = k }; next }
    /^ALLOW / { allowed[key($2, $3)] = 1; next }
    /^ +[^ ]+:[0-9]+-[0-9]+ .*<->/ { pair = $0; next }
    /Similarity:/ && pair != "" {
      k = pairkey(pair, 0)
      if (seen[pair]++) { pair = ""; next }
      found++; now[k] = 1
      split(pair, side, " <-> "); sub(/^ +/, "", side[1])
      split(side[1], a, " "); split(side[2], b, " ")
      if (!diffmode) { report("warning", a, b, $2, ""); pair = ""; next }
      if (!changed(a[1]) && changed(b[1])) { split(side[2], a, " "); split(side[1], b, " ") }
      touched = changed(a[1])
      if (k in allowed) { declared++; if (touched) report("warning", a, b, $2, " (declared)") }
      else if (k in base) { there++; if (touched) report("warning", a, b, $2, " (already there)") }
      else if (touched) { introduced++; report("error", a, b, $2, " (introduced)") }
      else { shifted++; report("error", a, b, $2, " (shifted)") }
      pair = ""
    }
    END {
      if (!diffmode) { printf "%d similar function pair(s)\n", found; exit 0 }
      for (k in base) if (!(k in now)) { contracted++; printf "contracted: %s\n", name[k] }
      printf "%d similar function pair(s): %d introduced, %d shifted, %d already there, %d declared; %d contracted\n", \
        found, introduced, shifted, there, declared, contracted
      if (introduced + shifted) {
        printf "%d new pair(s): reuse the existing function, extract a helper, or declare the pair in scripts/similar-code.allow\n", introduced + shifted
        exit 1
      }
    }
  '
