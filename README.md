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

The setup UI is implemented in Vue. The backend does not embed handwritten HTML.

Development flow:

```bash
cargo run
cd frontend
npm install
npm run dev
```

Open the Vite URL, usually `http://127.0.0.1:5173/`. If `.env` does not exist, the Vue app shows a step-by-step setup wizard and writes `.env` through `/api/setup` with:

- `SERVER_HOST` and `SERVER_PORT`
- `DATABASE_URL` for SQLite, MySQL, or PostgreSQL
- `JWT_SECRET`
- embedding and SiliconFlow settings
- optional reranker settings
- MCP enablement and auth requirement

Restart the backend after saving `.env`.

Production/static flow:

```bash
cd frontend
npm install
npm run build
cd ..
cargo run
```

The backend serves `frontend/dist` as static files and falls back to `frontend/dist/index.html` for the Vue app.

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

## Retrieval Configuration

Embedding is controlled by `EMBEDDING_PROVIDER`, `EMBEDDING_MODEL`, `SILICONFLOW_URL`, and `SILICONFLOW_API_KEY`.

Reranking is optional and controlled by:

- `RERANKER_ENABLED=true|false`
- `RERANKER_PROVIDER=local|siliconflow|custom_http`
- `RERANKER_MODEL=BAAI/bge-reranker-v2-m3`
- `RERANKER_URL=http://...` for custom HTTP reranker services

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
