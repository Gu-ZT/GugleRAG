# GugleRAG

GugleRAG is a self-hosted team knowledge base with Markdown documents, REST APIs, and an MCP JSON-RPC endpoint for AI agents.

## Current Structure

```text
.
├── src/                 # Rust backend MVP
├── frontend/            # Vue 3 + TypeScript + Vite frontend scaffold
├── .env.example         # Runtime configuration template
├── PLAN.md             # Product roadmap
└── AGENTS.md           # Agent/developer working notes
```

The backend currently keeps the MVP document store in `GUGLERAG_DATA` as JSON while the database layer is being initialized. Runtime configuration already accepts SQLite, MySQL, and PostgreSQL `DATABASE_URL` values so the SQLx persistence layer can be wired without changing the setup UX.

## First Run

If `.env` does not exist, open `http://127.0.0.1:8080/` after starting the backend. GugleRAG shows a setup page that writes `.env` with:

- `SERVER_HOST` and `SERVER_PORT`
- `DATABASE_URL` for SQLite, MySQL, or PostgreSQL
- `JWT_SECRET`
- embedding and SiliconFlow settings
- MCP enablement and auth requirement

Restart the backend after saving `.env`.

## Backend

```bash
cargo run
```

Useful endpoints:

- `GET /health`
- `GET /api/setup/status`
- `POST /api/setup`
- `POST /api/auth/register`
- `POST /api/auth/login`
- `GET/POST /api/documents`
- `GET/PUT/DELETE /api/documents/{id}`
- `GET /api/search?q=...`
- `POST /mcp`

## Frontend

```bash
cd frontend
npm install
npm run dev
```

The Vite dev server proxies `/api`, `/mcp`, and `/health` to `http://127.0.0.1:8080`.

## Database URLs

Supported URL prefixes:

- SQLite: `sqlite://data/guglerag.db`
- MySQL: `mysql://user:password@127.0.0.1:3306/guglerag`
- PostgreSQL: `postgresql://user:password@127.0.0.1:5432/guglerag`
