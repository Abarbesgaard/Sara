#!/usr/bin/env bash
# The primitives whose Contract or contract tests changed since the base, one
# per line; only those with a contract/<p>/README.md.
#
#   scripts/contract-primitives.sh origin/main
set -euo pipefail

base="${1:?give the base, e.g. origin/main}"
git diff --name-only "${base}..." -- contract tests/contract \
  | sed -nE 's#^contract/([^/]+)/.*#\1#p; s#^tests/contract/([^/]+)\.rs$#\1#p' \
  | sort -u | while read -r p; do
    if [ -f "contract/$p/README.md" ]; then echo "$p"; fi
  done
