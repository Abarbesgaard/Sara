# Adeptus Machinae — skills

Neo-Roman operating discipline for agents driving [sara](https://github.com/Abarbesgaard/Sara).
Each subfolder is a drop-in skill (`SKILL.md` with
frontmatter) for GitHub Copilot CLI / Claude.

| Skill | Kind | Invoke for |
|---|---|---|
| `adeptus` | creed | the adept's mindset — the five Leges (not agent law) |
| `legio` | faction | the operating law: order of authority, ground Leges, the three Maxims, the seven Viae |
| `via_genesis` | rite | build a new capability |
| `via_renovatio` | rite | improve without changing behaviour |
| `via_emendatio` | rite | repair a defect (test-first) |
| `via_restitutio` | rite | restore a broken build/deps/config (command-first) |
| `via_exploratio` | rite | investigate, no code changed |
| `via_validatio` | rite | write / run verification honestly |
| `via_publicatio` | rite | carry finished work to the gate |

## Install

Copy or symlink the skills into a skill root Copilot discovers
(`~/.copilot/skills`, `~/.agents/skills`):

```bash
for s in adeptus legio via_genesis via_renovatio via_emendatio via_restitutio \
         via_exploratio via_validatio via_publicatio; do
  ln -s "$PWD/skills/$s" "$HOME/.agents/skills/$s"
done
```

Then add the always-on hook to your project's `AGENTS.md` (see
`hook.AGENTS.md`) so an agent takes up the **Legio** faction at the start of
every charge. The creed (`adeptus`) is a human mindset — it is not loaded here.
`/env` should list the skills; revert by removing the symlinks.
