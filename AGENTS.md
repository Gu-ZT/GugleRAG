# AGENTS.md

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
- MCP exposes workspace and knowledge-base discovery tools. Every document/search MCP tool must require and validate both `workspace_id` and `knowledge_base_id` before touching a document.
- Keep `MCP_PUBLIC_URL` available for deployments where `SERVER_HOST` is a wildcard address or the service is behind a reverse proxy.
- Keep `src/main.rs` as a thin executable entry point. Backend composition belongs in `src/lib.rs`; configuration, persistence, domain models, authentication, REST APIs, MCP, and search belong in their dedicated modules.
- Put backend tests in the top-level `tests/` directory so they exercise the public library surface instead of being embedded in runtime modules.
- CI supports Linux x64/ARM64, Windows x64/ARM64, and macOS ARM64/x64 through native GitHub-hosted runners. Keep binary architecture verification and native server smoke tests enabled for every release target.
- Keep the release version synchronized across `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, and `frontend/package-lock.json`. Both changelogs require a matching `## [x.y.z]` section before tagging `vx.y.z`.
- Release artifacts are unsigned portable archives containing the executable and `frontend/dist`. Preserve that layout unless static assets become embedded in the executable, and keep generated `ci-output/` and `release/` directories untracked.
- Successful `main` pushes publish `v<manifest-version>-dev.<run_number>` prereleases after the reusable CI and all package jobs pass. Keep the release draft until every asset and the bilingual notes are ready; exact `v<manifest-version>` tags publish stable releases.

## Verification Commands

- Backend type check: `cargo check`
- Backend build: `cargo build`
- Backend formatting: `cargo fmt`
- Backend tests: `cargo test`
- Backend lint: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- Frontend dev server: `cd frontend && npm run dev`
- Frontend production build: `cd frontend && npm run build`
- Release tooling tests: `node --test ci/tests/*.test.mjs`
- Release version check: `node ci/check-version.mjs`
