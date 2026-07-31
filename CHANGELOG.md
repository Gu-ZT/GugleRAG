# Changelog

All notable changes to GugleRAG are documented in this file.

## [0.3.0] - 2026-07-31

- Replaced SQL JSON vector retrieval with embedded, knowledge-base-scoped Rust HNSW indexes that persist under `VECTOR_INDEX_PATH` and rebuild when stale.
- Migrated legacy chunk and one-vector SQL records into HNSW when compatible, with regeneration for documents whose chunk layout changed.

## [0.2.0] - 2026-07-31

- Added persistent, vendor-neutral document embeddings with cosine vector retrieval.
- Added SiliconFlow and local OpenAI-compatible embedding providers, with the complete default endpoint `https://api.siliconflow.cn/v1/embeddings`.
- Added optional SiliconFlow, local, and custom HTTP reranking providers.
- Added automatic startup and first-search indexing for documents created before this version, plus content-hash invalidation after edits.
- Added independent, expiring MCP access tokens with workspace scopes, listing, and revocation.
- Changed copied MCP configurations to the `http` type so clients can send `Authorization` headers.
- Updated the setup and administrator retrieval settings, integration coverage, and retrieval documentation.

## [0.1.1] - 2026-07-31

- Added administrator user management for listing users, reviewing their workspaces, and creating, updating, or deleting accounts.
- Added a public registration switch so deployments can disable self-service signup and require administrators to create users.
- Added administrator configuration and in-app service restart controls.
- Added database dialect support, document folders, and ZIP document import.
- Added cross-workspace knowledge-base search and improved workspace document tree handling.
- Added website icon support, pnpm package-manager metadata, expanded GitHub Markdown alerts and footnotes, and knowledge-base deletion.
- Added automatic restart after first-run setup writes `.env`.
- Added direct GitHub Release download links for generated release artifact and checksum entries.

## [0.1.0] - 2026-07-30

- Added the Rust and Vue 3 application structure with a step-by-step first-run setup flow.
- Added SQLite, MySQL, and PostgreSQL configuration through vendor-neutral SQLx queries.
- Added personal and team workspaces, multiple knowledge bases, invitations, and document editing.
- Added scoped MCP access with workspace and knowledge-base discovery tools.
- Added Markdown preview with notes, warnings, and GitHub-style task lists.
- Added architecture-verified portable CI and release packaging for Linux, Windows, and macOS on x64 and ARM64.
- Added automatic main-branch prereleases with idempotent build tags and stable version-tag releases.
