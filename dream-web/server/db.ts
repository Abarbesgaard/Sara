import { DatabaseSync } from "node:sqlite";
import { existsSync } from "node:fs";

/**
 * Open sara's SQLite database strictly read-only.
 *
 * `readOnly: true` means this process can never write to, migrate, or lock the
 * database that live sara agents may be actively using — the visualizer is a
 * pure observer. Any attempted write throws "attempt to write a readonly
 * database".
 */
export function openReadOnly(path: string): DatabaseSync {
  if (!existsSync(path)) {
    throw new Error(
      `sara database not found at:\n  ${path}\n` +
        `Set SARA_DB=/path/to/tasks.db or pass --db <path>.`,
    );
  }
  return new DatabaseSync(path, { readOnly: true });
}
