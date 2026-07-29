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
- Keep the workspace usable through normal account flows; do not require users to paste bearer tokens manually in the UI.
- Users have one personal workspace and may belong to multiple team workspaces. Knowledge bases are the document ownership boundary; document and search APIs must enforce knowledge-base access through workspace membership.
- Scoped MCP URLs are `/mcp/user/<token>`, `/mcp/group/<token>`, and `/mcp/all/<token>`. Validate both the path token and Bearer token, and keep MCP token values hashed at rest.
- Keep `MCP_PUBLIC_URL` available for deployments where `SERVER_HOST` is a wildcard address or the service is behind a reverse proxy.
- Keep `src/main.rs` as a thin executable entry point. Backend composition belongs in `src/lib.rs`; configuration, persistence, domain models, authentication, REST APIs, MCP, and search belong in their dedicated modules.
- Put backend tests in the top-level `tests/` directory so they exercise the public library surface instead of being embedded in runtime modules.

## Verification Commands

- Backend type check: `cargo check`
- Backend build: `cargo build`
- Backend formatting: `cargo fmt`
- Backend tests: `cargo test`
- Frontend dev server: `cd frontend && npm run dev`
- Frontend production build: `cd frontend && npm run build`
