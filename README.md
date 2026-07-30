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

The backend stores users, workspaces, teams, memberships, knowledge bases, documents, versions, and invitations through SQLx. Runtime configuration accepts SQLite, MySQL, and PostgreSQL `DATABASE_URL` values.

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
- optional `MCP_PUBLIC_URL` for reverse-proxy deployments

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
- `GET /api/workspaces`
- `GET/POST /api/workspaces/{workspace_id}/knowledge-bases`
- `GET/POST /api/teams`
- `GET /api/teams/{team_id}/members`
- `POST /api/teams/{team_id}/invitations`
- `GET /api/invitations`
- `POST /api/invitations/{token}/accept`
- `GET/POST /api/documents`
- `GET/PUT/DELETE /api/documents/{id}`
- `GET /api/search?q=...`
- `POST /mcp`
- `POST /mcp/all`
- `POST /mcp/{user|group}/{workspace_id}`

## Collaboration and MCP

Every user receives a personal workspace and its default knowledge base. Creating a team creates a team workspace and default knowledge base; team owners and admins can invite existing users by username. The invitation token can be shared with the invited user, who accepts it from the **Join team** dialog. A user may belong to multiple teams.

In the Vue workspace, use the selector at the top-left to switch between personal and team workspaces. Its adjacent `+` menu contains team creation, member invitation, and team joining actions. The sidebar renders every knowledge base in the selected workspace as a collapsible group with its articles nested underneath; new knowledge bases and articles can be created directly from that tree.

Documents belong to a knowledge base. Document and search requests accept `knowledge_base_id`; when it is omitted, the personal default knowledge base is used for backward compatibility.

The Vue workspace generates and copies stable MCP configurations through `POST /api/mcp/configs`. Personal and team configurations identify a workspace explicitly:

```json
{
  "scope": "user",
  "workspace_id": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
}
```

Use `scope: "group"` with a team `workspace_id`, or `scope: "all"` without `workspace_id` for every workspace the account can access. The response follows the requested streamable HTTP shape:

```json
{
  "type": "streamable-http",
  "url": "http://127.0.0.1:8080/mcp/user/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
  "headers": {
    "Authorization": "Bearer eyJhbGciOiJIUzI1NiJ9..."
  }
}
```

The UUID at the end of a personal or group URL is the workspace ID, not an access token. The all-workspaces URL is `/mcp/all` and has no trailing ID. Authorization reuses the user's current login JWT, so copying the same configuration repeatedly does not create or rotate credentials. JWT expiry and logout behavior remain the same as the normal account session, and workspace access is checked again on every MCP request. Set `MCP_PUBLIC_URL` when the server is behind a public hostname or reverse proxy.

MCP clients can discover resources before operating on documents:

- `list_workspaces()` returns the workspaces visible to the current MCP scope.
- `list_knowledge_bases(workspace_id)` returns the visible knowledge bases in that workspace.

Every document tool uses an explicit resource context. `search_knowledge`, `read_document`, `create_document`, `update_document`, `list_documents`, and `get_document_metadata` all require both `workspace_id` and `knowledge_base_id`; document-specific tools additionally require their existing `doc_id`, `folder_id`, or content fields. The server verifies that the knowledge base belongs to the workspace, both resources are available to the authenticated user, and the target document belongs to that knowledge base.

```json
{
  "name": "search_knowledge",
  "arguments": {
    "workspace_id": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
    "knowledge_base_id": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
    "query": "deployment"
  }
}
```

## Retrieval Configuration

Embedding is controlled by `EMBEDDING_PROVIDER`, `EMBEDDING_MODEL`, `SILICONFLOW_URL`, and `SILICONFLOW_API_KEY`.

Reranking is optional and controlled by:

- `RERANKER_ENABLED=true|false`
- `RERANKER_PROVIDER=local|siliconflow|custom_http`
- `RERANKER_MODEL=BAAI/bge-reranker-v2-m3`
- `RERANKER_URL=http://...` for custom HTTP reranker services

## Frontend

Markdown preview uses `markdown-it` with raw HTML disabled, followed by DOMPurify sanitization. It supports note/warning containers and read-only GitHub-style task lists:

```markdown
:::note
Deployment details belong here.
:::

:::warning Optional title
Check the production database before running this command.
:::

- [ ] Pending task
- [x] Completed task
```

Task checkboxes reflect the Markdown source and are intentionally disabled in preview mode.

```bash
cd frontend
npm install
npm run dev
```

The Vite dev server proxies `/api`, `/mcp`, and `/health` to `http://127.0.0.1:8080`.

The current Vue workspace supports:

- user registration and login
- token persistence in local storage
- personal and team workspace switching
- multiple knowledge bases per workspace
- team creation, member lists, invitations, and invitation acceptance
- document list, create, edit, save, delete
- tag editing
- keyword search over title, content, and tags
- edit/preview switching for Markdown text

## Database URLs

Supported URL prefixes:

- SQLite: `sqlite://data/guglerag.db?mode=rwc`
- MySQL: `mysql://user:password@127.0.0.1:3306/guglerag`
- PostgreSQL: `postgresql://user:password@127.0.0.1:5432/guglerag`

## CI and Releases

GitHub Actions validates the Rust backend, Vue frontend, release tooling, and a real HTTP server on every pull request and push to `main`. The supported release matrix is:

| Platform | Runner | Rust target | Archive |
| --- | --- | --- | --- |
| Linux x64 | `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Windows x64 | `windows-latest` | `x86_64-pc-windows-msvc` | `.zip` |
| macOS Apple Silicon | `macos-14` | `aarch64-apple-darwin` | `.tar.gz` |

Windows ARM64, Linux ARM64, and macOS x64 are not currently release targets because the repository does not yet have a native runner or verified cross-linking path for them.

Each archive is named `guglerag-v<version>-<platform>-<arch>.<format>` and has a matching `.sha256` file. It contains the server executable, `frontend/dist`, `.env.example`, both changelogs, this README, and `RELEASE-METADATA.json`. These are unsigned portable builds and must be extracted before running.

To publish a release:

1. Keep the version in `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, and `frontend/package-lock.json` synchronized.
2. Add matching `## [x.y.z]` sections to `CHANGELOG.md` and `CHANGELOG.zh-CN.md`.
3. Push the exact tag `vx.y.z`.

The release workflow validates all three native targets, creates portable archives and checksums, generates bilingual release notes, and publishes the draft only after every package succeeds. It uses the repository-provided `GITHUB_TOKEN`; no additional secrets or signing credentials are required.
