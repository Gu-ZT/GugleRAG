# GugleRAG

GugleRAG is a self-hosted team knowledge base with Markdown documents, REST APIs, and an MCP JSON-RPC endpoint for AI agents.

## Current Structure

```text
.
├── src/
│   ├── api/             # REST handlers grouped by responsibility
│   ├── mcp/             # MCP JSON-RPC endpoint and tools
│   ├── auth.rs          # JWT, password hashing, and account validation
│   ├── config.rs        # Runtime/setup configuration
│   ├── db.rs            # SQLx persistence
│   ├── domain.rs        # Shared domain models
│   ├── error.rs         # HTTP-aware application errors
│   ├── search.rs        # Keyword retrieval and ranking
│   ├── lib.rs           # Application composition
│   └── main.rs          # Thin executable entry point
├── tests/               # Backend integration tests
├── frontend/            # Vue 3 + TypeScript + Vite frontend
├── PLAN.md              # Product roadmap
└── AGENTS.md            # Agent/developer working notes
```

The backend stores users, documents, and document versions through SQLx. Runtime configuration accepts SQLite, MySQL, and PostgreSQL `DATABASE_URL` values.

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

Backend checks and tests:

```bash
cargo fmt -- --check
cargo check
cargo test
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

The current Vue workspace supports:

- user registration and login
- token persistence in local storage
- document list, create, edit, save, delete
- tag editing
- keyword search over title, content, and tags
- edit/preview switching for Markdown text

## Database URLs

Supported URL prefixes:

- SQLite: `sqlite://data/guglerag.db?mode=rwc`
- MySQL: `mysql://user:password@127.0.0.1:3306/guglerag`
- PostgreSQL: `postgresql://user:password@127.0.0.1:5432/guglerag`
