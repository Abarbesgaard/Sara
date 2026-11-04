#!/usr/bin/env bash
# The similar code already in the crate, grouped into clusters: functions joined
# by any chain of similar pairs from scripts/similar-code.sh. The PR gate only
# stops new pairs; this makes the existing ones a number to bring down.
#
#   scripts/similar-backlog.sh                 # print the report (markdown)
#   scripts/similar-backlog.sh --file-issues   # also file one issue per new cluster
#
# Nightly (.github/workflows/similar-backlog.yml) prints it to the job summary
# and files at most 3 issues a night, largest clusters first, label
# similar-code. An issue is never filed twice for the same cluster: closing one
# means "keep it"; declare its pairs in scripts/similar-code.allow.
set -euo pipefail

max_new_issues=3
dir="$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# "<file>::<fn><TAB><file>::<fn><TAB><similarity>" per pair, minus declared ones.
GITHUB_ACTIONS='' "$dir/similar-code.sh" | awk '
  match($0, /`[^`]+` is [0-9.]+% similar to `[^`]+` \([^:]+:/) {
    a = $0; sub(/:.*/, "", a)
    s = substr($0, RSTART, RLENGTH)
    split(s, q, "`"); pct = s; sub(/^.*` is /, "", pct); sub(/ similar.*/, "", pct)
    b = s; sub(/^.*\(/, "", b); sub(/:$/, "", b)
    print a "::" q[2] "\t" b "::" q[4] "\t" pct
  }' > "$work/pairs.tsv"
if [ -f "$dir/similar-code.allow" ]; then
  sed -e 's/#.*//' -e '/^[[:space:]]*$/d' "$dir/similar-code.allow" > "$work/allow.txt"
  awk -F"\t" -v allow="$work/allow.txt" 'FILENAME == allow { split($0, p, /[[:space:]]+/); ok[p[1] " " p[2]]; ok[p[2] " " p[1]]; next }
              !(($1 " " $2) in ok)' "$work/allow.txt" "$work/pairs.tsv" > "$work/live.tsv"
else
  cp "$work/pairs.tsv" "$work/live.tsv"
fi

# "<size><TAB><root><TAB><members, space-separated><TAB><pairs>" per cluster,
# largest first. The root is the cluster's first member in sort order, so a
# cluster keeps its name while it keeps that member.
awk -F'\t' '
  function find(x) { while (parent[x] != x) x = parent[x]; return x }
  function add(x) { if (!(x in parent)) parent[x] = x }
  { add($1); add($2); ra = find($1); rb = find($2); if (ra != rb) parent[ra < rb ? rb : ra] = (ra < rb ? ra : rb)
    pair[NR] = $1 "\t" $2 "\t" $3 }
  END {
    for (x in parent) { r = find(x); members[r] = members[r] " " x; n[r]++ }
    for (i = 1; i <= NR; i++) { split(pair[i], p, "\t"); r = find(p[1]); pairs[r] = pairs[r] "|" p[1] " ~ " p[2] " (" p[3] ")" }
    for (r in n) { root = r; split(substr(members[r], 2), ms, " "); for (i in ms) if (ms[i] < root) root = ms[i]
      print n[r] "\t" root "\t" substr(members[r], 2) "\t" substr(pairs[r], 2) }
  }' "$work/live.tsv" | sort -t$'\t' -k1,1nr -k2,2 > "$work/clusters.tsv"

pairs=$(wc -l < "$work/live.tsv" | tr -d ' ')
clusters=$(wc -l < "$work/clusters.tsv" | tr -d ' ')
echo "## Similar code"
echo
echo "**$pairs similar function pair(s) in $clusters cluster(s).** Bring this number down: extract a helper, or declare a pair that should stay in \`scripts/similar-code.allow\`."
echo
echo "| Functions | Pairs | Cluster |"
echo "| ---: | ---: | --- |"
awk -F'\t' '{ print "| " $1 " | " split($4, p, "|") " | `" $2 "` |" }' "$work/clusters.tsv"

[ "${1:-}" = --file-issues ] || exit 0
gh label create similar-code --color FBCA04 \
  --description "Functions that look alike; extract or declare them" 2>/dev/null || true
filed=0
while IFS=$'\t' read -r size root _ list; do
  [ "$filed" -lt "$max_new_issues" ] || break
  [ -z "$(gh issue list --label similar-code --state all --search "\"$root\" in:title" --json number -q '.[].number')" ] || continue
  n=$(printf '%s\n' "$list" | tr '|' '\n' | wc -l | tr -d ' ')
  body="These functions look alike: $n pair(s) found by similarity-rs via \`scripts/similar-code.sh\`."$'\n\n'
  body+=$(printf '%s\n' "$list" | tr '|' '\n' | head -40 | sed 's/^/- /')
  [ "$n" -le 40 ] || body+=$'\n'"- …and $((n - 40)) more"
  body+=$'\n\n'"Extract what they share, or declare the pairs that should stay apart in \`scripts/similar-code.allow\` and close this issue. Found by \`scripts/similar-backlog.sh\`."
  gh issue create --label similar-code --title "Similar code: \`$root\` ($size functions, $n pairs)" --body "$body" >/dev/null
  echo "Filed an issue for $root"
  filed=$((filed + 1))
done < "$work/clusters.tsv"
