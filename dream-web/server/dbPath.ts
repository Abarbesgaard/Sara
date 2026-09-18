import { homedir, platform } from "node:os";
import { join } from "node:path";
import { existsSync } from "node:fs";

/**
 * Resolve the path to sara's SQLite database.
 *
 * Precedence:
 *   1. `SARA_DB` environment variable (explicit override).
 *   2. `--db <path>` CLI argument.
 *   3. The platform default sara data directory.
 *
 * sara stores its DB under the OS application-data directory:
 *   - macOS:   ~/Library/Application Support/sara/tasks.db
 *   - Linux:   $XDG_DATA_HOME/sara/tasks.db or ~/.local/share/sara/tasks.db
 *   - Windows: %APPDATA%/sara/tasks.db
 */
export function resolveDbPath(argv: string[] = process.argv): string {
  const fromEnv = process.env.SARA_DB;
  if (fromEnv && fromEnv.trim()) return fromEnv.trim();

  const flagIndex = argv.indexOf("--db");
  if (flagIndex !== -1 && argv[flagIndex + 1]) return argv[flagIndex + 1];

  for (const candidate of defaultDbCandidates()) {
    if (existsSync(candidate)) return candidate;
  }
  // Return the most-likely default even if missing, so the error message points
  // at the expected location.
  return defaultDbCandidates()[0];
}

function defaultDbCandidates(): string[] {
  const home = homedir();
  const os = platform();
  const candidates: string[] = [];

  if (os === "darwin") {
    candidates.push(join(home, "Library", "Application Support", "sara", "tasks.db"));
  } else if (os === "win32") {
    const appData = process.env.APPDATA ?? join(home, "AppData", "Roaming");
    candidates.push(join(appData, "sara", "tasks.db"));
  } else {
    const xdg = process.env.XDG_DATA_HOME ?? join(home, ".local", "share");
    candidates.push(join(xdg, "sara", "tasks.db"));
  }
  // Cross-platform fallbacks, in case the data dir differs.
  candidates.push(join(home, ".local", "share", "sara", "tasks.db"));
  candidates.push(join(home, "Library", "Application Support", "sara", "tasks.db"));
  return candidates;
}
