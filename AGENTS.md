# AGENTS.md

1. Prefer self-explanatory code, clear names, and small focused functions.
2. Avoid comments that merely restate what the code already says.
3. Add comments only when they explain non-obvious intent, constraints, or tradeoffs.
4. Except for utility functions, each source file must declare only one class. Multiple classes in the same file are
   strictly forbidden; a class's `companion object` and/or inner class (es) may remain in the same file.
5. Wildcard imports such as `import package.*` are forbidden. Always use explicit imports.
6. Fully qualified names are forbidden in source code. Use explicit imports and avoid introducing conflicting simple
   names in the same context whenever possible.

## Source Lookup Rules

1. Use CLI tools for reading project files and `rg` for searching project file names or contents.
2. Do not use IDEA MCP tools or the `workspace-agent-bridge` skill for project file lookup or content search.
3. When project source lookup cannot be satisfied locally, search source JARs under `~/.gradle/caches/`.

## Tool Preference For Other Work

For tasks other than searching or reading project files, prefer IDEA MCP tools and the `workspace-agent-bridge` skill
when they provide the relevant operation, such as diagnostics, formatting, refactoring, or other IDE-aware actions.

## Verification And Runtime Rules

1. Run Rust and frontend verification commands through RustRover MCP or the `workspace-agent-bridge` skill. This includes type checks, formatting, builds, tests, linting, and development servers.
2. Do not start `cargo`, `pnpm`, `node`, or other project verification/runtime commands directly from PowerShell or another shell. If the IDE bridge is unavailable for this project, report the blocker instead of falling back to a shell command.

## Current Project Notes

- The repository is a Rust backend at the root plus a Vue 3/Vite frontend in `frontend/`.
- Setup UI must be implemented in Vue under `frontend/`; do not add embedded HTML strings to Rust handlers.
- Setup should remain a step-by-step wizard, not a single long all-fields form.
- Keep reranker config support in setup and `.env`: `RERANKER_ENABLED`, `RERANKER_PROVIDER`, `RERANKER_MODEL`, `RERANKER_URL`.
- The backend serves `frontend/dist` static files in production and falls back to `frontend/dist/index.html` for SPA routes.
- If `.env` is missing, the Vue app must show the setup UI and `/api/setup` may write the first `.env`. Once `.env` exists, setup writes are rejected.
- `DATABASE_URL` must remain database-vendor neutral. Keep support for SQLite, MySQL, and PostgreSQL URL prefixes when adding SQLx persistence.
- Users, documents, and document versions are persisted through SQLx. Keep query code database-vendor neutral.
- Do not commit `.env`, local data files, `target/`, `frontend/node_modules/`, or `frontend/dist/`.
- Frontend development should happen in `frontend/` with Vue 3, TypeScript, and Vite. The Vite proxy expects the backend on `127.0.0.1:8080`.
- Keep the workspace selector at the top-left. Its adjacent `+` menu owns team creation, member invitation, and team joining; the main sidebar stays focused on collapsible knowledge-base groups and their articles.
- Use `@lucide/vue` for interface icons instead of adding new inline SVG markup.
- Keep Markdown preview on `markdown-it` with raw HTML disabled and sanitize rendered output with DOMPurify. Preserve `:::note`, `:::warning`, and read-only GitHub-style task-list support when changing the renderer.
- Keep the workspace usable through normal account flows; do not require users to paste bearer tokens manually in the UI.
- Users have one personal workspace and may belong to multiple team workspaces. Knowledge bases are the document ownership boundary; document and search APIs must enforce knowledge-base access through workspace membership.
- MCP configs reuse the current login JWT. Workspace-scoped URLs are `/mcp/user/<workspace_id>` and `/mcp/group/<workspace_id>`; `/mcp/all` has no resource ID and covers every workspace available to the user. Do not create separate MCP credentials when copying a config.
- MCP exposes workspace and knowledge-base discovery tools. Document MCP tools must require and validate both `workspace_id` and `knowledge_base_id` before touching a document. `search_knowledge` may accept omitted, single, or array resource IDs, but must resolve them only within the current MCP scope and validate every resulting knowledge base before loading documents.
- Keep `MCP_PUBLIC_URL` available for deployments where `SERVER_HOST` is a wildcard address or the service is behind a reverse proxy.
- Keep `src/main.rs` as a thin executable entry point. Backend composition belongs in `src/lib.rs`; configuration, persistence, domain models, authentication, REST APIs, MCP, and search belong in their dedicated modules.
- Put backend tests in the top-level `tests/` directory so they exercise the public library surface instead of being embedded in runtime modules.
- CI supports Linux x64/ARM64, Windows x64/ARM64, and macOS ARM64/x64 through native GitHub-hosted runners. Keep binary architecture verification and native server smoke tests enabled for every release target.
- Keep the release version synchronized across `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, and `frontend/package-lock.json`. Both changelogs require a matching `## [x.y.z]` section before tagging `vx.y.z`.
- Release artifacts are unsigned portable archives containing the executable and `frontend/dist`. Preserve that layout unless static assets become embedded in the executable, and keep generated `ci-output/` and `release/` directories untracked.
- Successful `main` pushes publish `v<manifest-version>-dev.<run_number>` prereleases after the reusable CI and all package jobs pass. Keep the release draft until every asset and the bilingual notes are ready; exact `v<manifest-version>` tags publish stable releases.

## Verification Commands

Run the following through RustRover MCP or `workspace-agent-bridge`, never directly from PowerShell.

- Backend type check: `cargo check`
- Backend build: `cargo build`
- Backend formatting: `cargo fmt`
- Backend tests: `cargo test`
- Backend lint: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- Frontend dev server: `cd frontend && pnpm run dev`
- Frontend production build: `cd frontend && pnpm run build`
- Release tooling tests: `node --test ci/tests/*.test.mjs`
- Release version check: `node ci/check-version.mjs`
