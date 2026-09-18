#!/usr/bin/env bash
# pulse-demo.sh — make the dream-web brain light up on demand.
#
# Emits synthetic `memory_recalled` events into sara's event log every few
# seconds for ~2 minutes, in small "activation cascades" (a seed memory plus a
# couple of its linked neighbours) so you can watch nodes fire, signal particles
# travel along the bonds, and shockwave rings ripple out — exactly the path a
# real `recall` drives, just on a timer.
#
# The viewer reads the DB read-only (WAL), so this writer never blocks it. All
# events are tagged project='__dream_demo__' and DELETED on exit (normal end or
# Ctrl-C), leaving the real memory graph untouched.
#
# Usage:  bash scripts/pulse-demo.sh [DURATION_SECONDS] [SEED_INTERVAL_SECONDS]
#           DURATION        total run time      (default 120)
#           SEED_INTERVAL   gap between cascades (default 5)

set -u

DURATION="${1:-120}"
INTERVAL="${2:-5}"
TAG="__dream_demo__"

DB="${SARA_DB:-$HOME/Library/Application Support/sara/tasks.db}"
if [[ ! -f "$DB" ]]; then
  echo "sara DB not found: $DB (set SARA_DB to override)" >&2
  exit 1
fi

sq() { sqlite3 "$DB" ".timeout 4000" "$1"; }

cleanup() {
  local n
  n=$(sq "SELECT count(*) FROM events WHERE project='$TAG';")
  sq "DELETE FROM events WHERE project='$TAG';" >/dev/null 2>&1
  echo ""
  echo "cleaned up $n demo event(s) — real memory graph untouched."
}
trap cleanup EXIT INT TERM

# Escape single quotes for safe inlining into SQL.
esc() { printf "%s" "$1" | sed "s/'/''/g"; }

# fire <uuid> [event_action]
#   event_action defaults to memory_recalled. The viewer's /api/activity feed
#   maps event names → brain actions/colours:
#     memory_recalled → recall (white)   memory_learned → learn (green)
#     memory_surfaced → surface (blue)   memory_linked  → link  (cyan)
#     task_touched    → task   (amber)
fire() {
  local uuid; uuid="$(esc "$1")"
  local action="${2:-memory_recalled}"
  sq "INSERT INTO events(action, ref_uuid, kind, tags_json, project, at)
      VALUES('$action', '$uuid', 'memory', '[\"demo\"]', '$TAG',
             strftime('%Y-%m-%dT%H:%M:%f','now') || '+00:00');" >/dev/null
}

# A visible memory's bonded neighbours (either direction), visible-only.
neighbours() {
  local uuid; uuid="$(esc "$1")"
  sq "SELECT u FROM (
        SELECT to_uuid AS u FROM memory_links WHERE from_uuid='$uuid'
        UNION
        SELECT from_uuid AS u FROM memory_links WHERE to_uuid='$uuid'
      )
      JOIN items it ON it.uuid = u
      WHERE it.kind='memory' AND it.status IN ('active','provisional')
      ORDER BY random() LIMIT 2;"
}

echo "dream-web pulse demo → firing brain activity into $DB"
echo "  duration ${DURATION}s · cascade every ~${INTERVAL}s · viewer: http://127.0.0.1:5173"
echo "  actions: recall·learn·surface·task on the seed, link on its neighbours"
echo "  (Ctrl-C to stop early; demo events auto-clean on exit)"
echo ""

# Seed actions cycle so you see every colour over a run.
ACTIONS=(memory_recalled memory_learned memory_surfaced task_touched memory_recalled)
END=$(( $(date +%s) + DURATION ))
count=0
i=0
while [[ $(date +%s) -lt $END ]]; do
  seed="$(sq "SELECT uuid FROM items
              WHERE kind='memory' AND status IN ('active','provisional')
              ORDER BY random() LIMIT 1;")"
  [[ -z "$seed" ]] && break

  action="${ACTIONS[$(( i % ${#ACTIONS[@]} ))]}"
  i=$((i + 1))
  fire "$seed" "$action"
  count=$((count + 1))
  label="$(sq "SELECT 'm' || COALESCE(display_id,0) FROM items WHERE uuid='$(esc "$seed")';")"
  echo "  [$(date +%H:%M:%S)] ${action#memory_} → $label + link neighbours"

  # Stagger the neighbours as a spreading synapse (link = cyan).
  while IFS= read -r nb; do
    [[ -z "$nb" ]] && continue
    sleep 1
    fire "$nb" memory_linked
    count=$((count + 1))
  done < <(neighbours "$seed")

  sleep "$INTERVAL"
done

echo ""
echo "done — emitted $count activity event(s) over ${DURATION}s."
