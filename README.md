<div align="center">

<img src=".idea/icon.png" width="256" height="256" alt="GugleRAG icon">

# GugleRAG

**A self-hosted team knowledge base for people and AI agents.**

English | [简体中文](README.zh_CN.md)

</div>

GugleRAG is a self-hosted team knowledge base with Markdown documents, REST APIs, and an MCP JSON-RPC endpoint for AI
agents.

## Screenshots

### First-run Setup

| Service                                                                      | Database                                                                      |
|------------------------------------------------------------------------------|-------------------------------------------------------------------------------|
| <img src="docs/init-01.jpeg" alt="First-run setup service step" width="420"> | <img src="docs/init-02.jpeg" alt="First-run setup database step" width="420"> |

| Retrieval                                                                      | MCP                                                                      |
|--------------------------------------------------------------------------------|--------------------------------------------------------------------------|
| <img src="docs/init-03.jpeg" alt="First-run setup retrieval step" width="420"> | <img src="docs/init-04.jpeg" alt="First-run setup MCP step" width="420"> |

### Account Flow

| Login                                                      | Registration                                                         |
|------------------------------------------------------------|----------------------------------------------------------------------|
| <img src="docs/login.jpeg" alt="Login screen" width="420"> | <img src="docs/register.jpeg" alt="Registration screen" width="420"> |

### Knowledge Workspace

| Workspace                                                                | Create Document                                                           |
|--------------------------------------------------------------------------|---------------------------------------------------------------------------|
| <img src="docs/empty-page.jpeg" alt="Workspace empty state" width="420"> | <img src="docs/create_doc.jpeg" alt="Create document dialog" width="420"> |

| Edit Document                                                    | Preview Document                                                              |
|------------------------------------------------------------------|-------------------------------------------------------------------------------|
| <img src="docs/edit_doc.jpeg" alt="Document editor" width="420"> | <img src="docs/preview_doc.jpeg" alt="Markdown document preview" width="420"> |

### Collaboration and Administration

| Create Team                                                            | Join Team                                                          |
|------------------------------------------------------------------------|--------------------------------------------------------------------|
| <img src="docs/create_team.jpeg" alt="Create team dialog" width="420"> | <img src="docs/join_team.jpeg" alt="Join team dialog" width="420"> |

| Service Settings                                                                      | User Management                                                                       |
|---------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------|
| <img src="docs/admin_settings.jpeg" alt="Administrator service settings" width="420"> | <img src="docs/user-management.jpeg" alt="Administrator user management" width="420"> |

| MCP Configuration                                                    |
|----------------------------------------------------------------------|
| <img src="docs/mcp.jpeg" alt="MCP configuration dialog" width="840"> |

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
│   ├── embedding.rs     # Embedding provider clients
│   ├── logging.rs       # Rolling file and console logging
│   ├── reranker.rs      # Optional reranking provider clients
│   ├── search.rs        # Persistent vector retrieval and ranking
│   ├── lib.rs           # Application composition
│   └── main.rs          # Thin executable entry point
├── tests/               # Backend integration tests
├── frontend/            # Vue 3 + TypeScript + Vite frontend
├── PLAN.md              # Product roadmap
└── AGENTS.md            # Agent/developer working notes
```

The backend stores users, workspaces, teams, memberships, knowledge bases, documents, versions, invitations, and
document embeddings through SQLx. Runtime configuration accepts SQLite, MySQL, and PostgreSQL `DATABASE_URL` values. The
`document_embeddings` table is a vendor-neutral persistent vector index: vectors are stored as JSON with their provider,
model, dimension, and content hash, then cosine similarity is calculated by Rust.

## First Run

The setup UI is implemented in Vue. The backend does not embed handwritten HTML.

Development flow:

```bash
cargo run
cd frontend
npm install
npm run dev
```

Open the Vite URL, usually `http://127.0.0.1:5173/`. If `.env` does not exist, the Vue app shows a step-by-step setup
wizard and writes `.env` through `/api/setup` with:

- `SERVER_HOST` and `SERVER_PORT`
- `DATABASE_URL` for SQLite, MySQL, or PostgreSQL
- `JWT_SECRET`
- embedding, complete SiliconFlow endpoint, and model settings
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

## Logging

The server writes structured logs to both the console and `logs/latest.log`. At process startup, a non-empty previous
`latest.log` is compressed into `logs/log-YY-MM-dd-HH:mm:ss:ms.log.gz` and a new `latest.log` is created. The active
file rolls before a write would exceed 500 KiB. Windows does not allow colons in filenames, so Windows archives use
`log-YY-MM-dd-HH-mm-ss-ms.log.gz` instead.

Useful endpoints:

- `GET /health`
- `GET /api/setup/status`
- `POST /api/setup`
- `POST /api/auth/register`
- `POST /api/auth/login`
- `GET/PUT /api/admin/config` (administrator only)
- `POST /api/admin/restart` (administrator only)
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

Every user receives a personal workspace and its default knowledge base. Creating a team creates a team workspace and
default knowledge base; team owners and admins can invite existing users by username. The invitation token can be shared
with the invited user, who accepts it from the **Join team** dialog. A user may belong to multiple teams.

In the Vue workspace, use the selector at the top-left to switch between personal and team workspaces. Its adjacent `+`
menu contains team creation, member invitation, and team joining actions. The sidebar renders every knowledge base in
the selected workspace as a collapsible group with its articles nested underneath; new knowledge bases and articles can
be created directly from that tree.

Documents belong to a knowledge base. Document and search requests accept `knowledge_base_id`; when it is omitted, the
personal default knowledge base is used for backward compatibility.

The Vue workspace generates and copies stable MCP configurations through `POST /api/mcp/configs`. Personal and team
configurations identify a workspace explicitly:

```json
{
  "scope": "user",
  "workspace_id": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
}
```

Use `scope: "group"` with a team `workspace_id`, or `scope: "all"` without `workspace_id` for every workspace the
account can access. The response follows the requested streamable HTTP shape:

```json
{
  "type": "streamable-http",
  "url": "http://127.0.0.1:8080/mcp/user/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
  "headers": {
    "Authorization": "Bearer eyJhbGciOiJIUzI1NiJ9..."
  }
}
```

The UUID at the end of a personal or group URL is the workspace ID, not an access token. The all-workspaces URL is
`/mcp/all` and has no trailing ID. Authorization reuses the user's current login JWT, so copying the same configuration
repeatedly does not create or rotate credentials. JWT expiry and logout behavior remain the same as the normal account
session, and workspace access is checked again on every MCP request. Set `MCP_PUBLIC_URL` when the server is behind a
public hostname or reverse proxy.

MCP clients can discover resources before operating on documents:

- `list_workspaces()` returns the workspaces visible to the current MCP scope.
- `list_knowledge_bases(workspace_id)` returns the visible knowledge bases in that workspace.

Document read/write/list tools require one explicit `workspace_id` and `knowledge_base_id`; document-specific tools
additionally require their existing `doc_id`, `folder_id`, or content fields. `search_knowledge` accepts either one UUID
or an array of UUIDs for each resource parameter. Omit `workspace_id` to search every workspace visible to the current
MCP scope, omit `knowledge_base_id` to search every knowledge base in the selected workspaces, or omit both to search
every accessible knowledge base. Every explicit ID is still validated against the MCP scope and knowledge-base
ownership. Search results include `workspace_id` and `knowledge_base_id` so a result can be used with a document tool.

```json
{
  "name": "search_knowledge",
  "arguments": {
    "workspace_id": [
      "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
      "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"
    ],
    "knowledge_base_id": [
      "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
    ],
    "query": "deployment"
  }
}
```

## Retrieval Configuration

Embedding and vector indexing are controlled by:

- `EMBEDDING_PROVIDER=stub|local|siliconflow`
- `EMBEDDING_MODEL=BAAI/bge-m3`
- `EMBEDDING_URL=https://api.siliconflow.cn/v1/embeddings`
- `SILICONFLOW_URL=https://api.siliconflow.cn`
- `SILICONFLOW_API_KEY=sk-...` for SiliconFlow embeddings or reranking

The setup wizard defaults to SiliconFlow with `BAAI/bge-m3`. The default embedding request URL is the complete
`https://api.siliconflow.cn/v1/embeddings` endpoint; `SILICONFLOW_URL` remains the API base used to derive the reranker
endpoint. `local` uses the configured `EMBEDDING_URL` as an OpenAI-compatible HTTP endpoint. `stub` is a deterministic
offline provider intended for tests and installations that are not ready to call a model service.

For every non-folder document, GugleRAG embeds the title, tags, and content, stores the vector in
`document_embeddings`, and reuses it while the document content hash and provider/model settings remain unchanged. The
server rebuilds missing or stale vectors at startup; a first search also performs lazy indexing, so documents from
versions before 0.2.0 are migrated without a separate export step.

Reranking is optional and controlled by:

- `RERANKER_ENABLED=true|false`
- `RERANKER_PROVIDER=local|siliconflow|custom_http`
- `RERANKER_MODEL=BAAI/bge-reranker-v2-m3`
- `RERANKER_URL=http://...` for local or custom HTTP reranker services

The SiliconFlow reranker uses `SILICONFLOW_URL/v1/rerank`. Local and custom HTTP rerankers use `RERANKER_URL`. Each
reranker receives `{ model, query, documents, top_n, return_documents: false }` and may return `results` or
`data` entries containing `index` and `score` or `relevance_score`.

## Frontend

Markdown preview uses `markdown-it` with raw HTML disabled, followed by DOMPurify sanitization. It supports note/warning
containers and read-only GitHub-style task lists:

```markdown
:::note Deployment details belong here.
:::

:::warning Optional title Check the production database before running this command.
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
- persistent embedding search over title, content, and tags, with optional model reranking
- edit/preview switching for Markdown text

## Database URLs

Supported URL prefixes:

- SQLite: `sqlite://data/guglerag.db?mode=rwc`
- MySQL: `mysql://user:password@127.0.0.1:3306/guglerag`
- PostgreSQL: `postgresql://user:password@127.0.0.1:5432/guglerag`

## CI and Releases

GitHub Actions validates the Rust backend, Vue frontend, release tooling, and a real HTTP server on every pull request
and push to `main`. Pull requests run CI without publishing. A successful `main` build publishes a prerelease after all
six native packages succeed. The supported release matrix is:

| Platform            | Runner             | Rust target                 | Archive   |
|---------------------|--------------------|-----------------------------|-----------|
| Linux x64           | `ubuntu-24.04`     | `x86_64-unknown-linux-gnu`  | `.tar.gz` |
| Linux ARM64         | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| Windows x64         | `windows-latest`   | `x86_64-pc-windows-msvc`    | `.zip`    |
| Windows ARM64       | `windows-11-arm`   | `aarch64-pc-windows-msvc`   | `.zip`    |
| macOS Apple Silicon | `macos-15`         | `aarch64-apple-darwin`      | `.tar.gz` |
| macOS Intel         | `macos-15-intel`   | `x86_64-apple-darwin`       | `.tar.gz` |

Every target uses a matching native GitHub-hosted runner. CI parses each ELF, PE, or Mach-O header to verify the
packaged CPU architecture and then starts that binary for a server smoke test.

Each archive is named `guglerag-v<version>-<platform>-<arch>.<format>` and has a matching `.sha256` file. It contains
the server executable, `frontend/dist`, `.env.example`, both changelogs, this README, and `RELEASE-METADATA.json`. These
are unsigned portable builds and must be extracted before running.

Main-branch prereleases use `v<manifest-version>-dev.<run_number>`, for example `v0.1.0-dev.42`. A rerun keeps the same
GitHub run number and reuses the same release instead of creating a duplicate. The release remains a draft while
packages are uploading and becomes visible as a prerelease only after every target and the bilingual release notes
succeed.

To publish a stable release:

1. Keep the version in `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, and `frontend/package-lock.json`
   synchronized.
2. Add matching `## [x.y.z]` sections to `CHANGELOG.md` and `CHANGELOG.zh-CN.md`.
3. Push the exact tag `vx.y.z`.

The release workflow validates all six native targets, creates portable archives and checksums, generates bilingual
release notes, and publishes the draft only after every package succeeds. Stable releases use the exact manifest
version; prerelease artifacts append the CI build identifier while reading notes from the matching base-version
changelog section. The workflow uses the repository-provided `GITHUB_TOKEN`; no additional secrets or signing
credentials are required.
