#!/usr/bin/env python3
"""Mine a recall eval set from a sara DB (read-only).

Two sources of ground truth, written as JSONL (one query per line):

  cited   query = a task's description, relevant = memories that task cited.
          `cutoff` = task creation time: only memories that existed before the
          task are fair answers (memories learned *from* the task would leak).
  linked  query = a memory's title, relevant = memories it is linked to with
          derived_from / similar_to / supersedes. The query memory itself is
          excluded from the corpus. Capped per target so hub canonicals (m61
          has 60 children) don't dominate.

Usage: mine_eval.py [DB] > eval.jsonl   (default: the live sara DB)
"""
import json, os, sqlite3, sys
from collections import defaultdict

db = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
    "~/Library/Application Support/sara/tasks.db")
con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
PER_TARGET_CAP = 3

cited = defaultdict(set)
meta = {}
for t_uuid, desc, entry, i_uuid in con.execute("""
    select t.uuid, t.description, t.entry, i.uuid
      from memory_uses u
      join tasks t on t.uuid = u.task_uuid
      join items i on i.uuid = u.item_uuid
     where u.kind = 'cited' and i.status = 'active' and i.created < t.entry
       and (i.source_task_uuid is null or i.source_task_uuid != t.uuid)"""):
    cited[t_uuid].add(i_uuid)
    meta[t_uuid] = (desc, entry)
for t_uuid, rel in sorted(cited.items()):
    desc, entry = meta[t_uuid]
    print(json.dumps({"source": "cited", "query": desc.strip(), "relevant": sorted(rel),
                      "cutoff": entry, "exclude": []}))

per_target = defaultdict(int)
linked = defaultdict(set)
titles = {}
for f_uuid, f_title, t_uuid in con.execute("""
    select a.uuid, a.title, b.uuid
      from memory_links l
      join items a on a.uuid = l.from_uuid
      join items b on b.uuid = l.to_uuid
     where a.status = 'active' and b.status = 'active'
       and l.relation in ('derived_from', 'similar_to', 'supersedes')
     order by l.id"""):
    if per_target[t_uuid] >= PER_TARGET_CAP:
        continue
    per_target[t_uuid] += 1
    linked[f_uuid].add(t_uuid)
    titles[f_uuid] = f_title
for f_uuid, rel in sorted(linked.items()):
    print(json.dumps({"source": "linked", "query": titles[f_uuid].strip(), "relevant": sorted(rel),
                      "cutoff": None, "exclude": [f_uuid]}))
