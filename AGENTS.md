# AGENTS.md

## Current Project Notes

- The repository is a Rust backend at the root plus a Vue 3/Vite frontend in `frontend/`.
- If `.env` is missing, `/` must show the setup UI and `/api/setup` may write the first `.env`. Once `.env` exists, setup writes are rejected.
- `DATABASE_URL` must remain database-vendor neutral. Keep support for SQLite, MySQL, and PostgreSQL URL prefixes when adding SQLx persistence.
- The current MVP still stores documents/users in `GUGLERAG_DATA` JSON. Replace this through a repository layer instead of coupling handlers directly to SQLx.
- Do not commit `.env`, local data files, `target/`, `frontend/node_modules/`, or `frontend/dist/`.
- Frontend development should happen in `frontend/` with Vue 3, TypeScript, and Vite. The Vite proxy expects the backend on `127.0.0.1:8080`.

## Verification Commands

- Backend type check: `cargo check`
- Backend build: `cargo build`
- Backend formatting: `cargo fmt`
- Frontend dev server: `cd frontend && npm run dev`
- Frontend production build: `cd frontend && npm run build`
