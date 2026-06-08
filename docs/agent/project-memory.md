# Project Memory and References

Use this document as the map for project memory sources. Read only what matches the task.

## Source Priority

When sources disagree, use this order:

1. Current code
2. `VISION.md` — 产品方向与底线（长期稳定，极少改动）
3. Root `AGENTS.md` navigation rules
4. Focused docs referenced by `AGENTS.md`
5. `docs/` current-state guides
6. `.planning/` roadmap and spike notes
7. DeepWiki / external references

## Project Memory Files

### Planning state

- `.planning/PROJECT.md`
- `.planning/REQUIREMENTS.md`
- `.planning/ROADMAP.md`
- `.planning/STATE.md`

Read only when the task is about roadmap, planning, or requirement alignment.

## Existing Documentation Map

### High-level project docs

- `docs/README.md` — doc index
- `docs/overview.md` — product/system overview
- `README.md` / `README_ZH.md` — public project introduction
- `docs/development/config.md` — current default data/log paths across macOS, Linux, and Windows

### Architecture docs

Read these before structural work:

- `docs/architecture/principles.md`
- `docs/architecture/module-boundaries.md`
- `docs/architecture/bootstrap.md`
- `docs/guides/error-handling.md`

### Area-specific local guides

- `src/AGENTS.md` — frontend-local map
- `src-tauri/AGENTS.md` — Rust/Tauri workspace-local map

## External Reference

### DeepWiki

Project architecture reference:

- URL: `https://deepwiki.com/UniClipboard/UniClipboard`
- Access: use the DeepWiki MCP server when the task needs diagrams, historical architecture context, or flow explanations that are not obvious from code.

Do not treat DeepWiki as a higher authority than the repository code.
