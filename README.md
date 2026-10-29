# brainscan

A terminal UI to browse the files that drive LLM agents in a repo: instructions, memory, settings, skills, commands and so on. Files are listed by agent and kind on the left, with a syntax-highlighted preview on the right.

## Install

```sh
cargo install --path .
```

## Usage

```sh
brainscan [DIR]
```

`DIR` defaults to the current directory. The walk respects `.gitignore`, except for `CLAUDE.local.md` and `.claude/settings.local.json`, which are picked up next to the other files found.

| Key | Action |
|-----|--------|
| `j` / `k`, arrows | Select next / previous file |
| `PgDn` / `PgUp` | Move selection by a page |
| `g` / `G`, `Home` / `End` | First / last file |
| `J` / `K` | Scroll preview by a line |
| `Ctrl-d` / `Ctrl-u` | Scroll preview by a page |
| `e` | Open in `$VISUAL`, `$EDITOR` or `vi` |
| `q`, `Esc` | Quit |

## Detected files

| Agent | Kind | Files |
|-------|------|-------|
| agents.md | Instructions | `AGENTS.md` |
| claude | Memory | `CLAUDE.md`, `CLAUDE.local.md` |
| claude | MCP | `.mcp.json` |
| claude | Settings | `.claude/settings.json`, `.claude/settings.local.json` |
| claude | Rules | `.claude/rules/**/*.md` |
| claude | Skills | `.claude/skills/<name>/**` |
| claude | Commands | `.claude/commands/**/*.md` |
| claude | Subagents | `.claude/agents/**/*.md` |
| claude | Output styles | `.claude/output-styles/*.md` |

Nested `.claude` directories (e.g. in monorepos) are detected too. Add an agent by adding match arms to `classify` in `src/discover.rs`.
