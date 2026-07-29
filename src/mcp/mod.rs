use crate::{
    AppState,
    auth::{self, hash_access_token, hash_password},
    domain::{Document, DocumentVersion, KnowledgeBase, Role, User},
    error::AppError,
    search,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/mcp", post(mcp_endpoint))
        .route("/mcp/{scope}/{token}", post(scoped_mcp_endpoint))
}

async fn mcp_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !state.config.mcp_enabled {
        return Json(json_rpc_error(id, -32000, "MCP endpoint disabled"));
    }
    let user_id = match auth::require_user(&headers, &state).await {
        Ok(user_id) => Some(user_id),
        Err(_) if state.config.mcp_auth_required => {
            return Json(json_rpc_error(id, -32001, "authentication required"));
        }
        Err(_) => None,
    };
    let knowledge_bases = match user_id {
        Some(user_id) => match state.database.accessible_knowledge_bases(user_id).await {
            Ok(knowledge_bases) => Some(knowledge_bases),
            Err(error) => return Json(json_rpc_error(id, -32000, &error.to_string())),
        },
        None => None,
    };
    mcp_response(&state, request, user_id, knowledge_bases.as_deref()).await
}

async fn scoped_mcp_endpoint(
    State(state): State<AppState>,
    Path((scope, raw_token)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !state.config.mcp_enabled {
        return Json(json_rpc_error(id, -32000, "MCP endpoint disabled"));
    }
    let bearer = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if bearer != Some(raw_token.as_str()) {
        return Json(json_rpc_error(id, -32001, "invalid MCP authorization"));
    }
    let token = match state
        .database
        .find_mcp_token(&hash_access_token(&raw_token))
        .await
    {
        Ok(Some(token)) if token.scope == scope => token,
        Ok(_) => return Json(json_rpc_error(id, -32001, "invalid MCP token")),
        Err(error) => return Json(json_rpc_error(id, -32000, &error.to_string())),
    };
    let knowledge_bases = match knowledge_bases_for_token(&state, &token).await {
        Ok(knowledge_bases) => knowledge_bases,
        Err(error) => return Json(json_rpc_error(id, -32001, &error.to_string())),
    };
    mcp_response(
        &state,
        request,
        Some(token.user_id),
        Some(knowledge_bases.as_slice()),
    )
    .await
}

async fn mcp_response(
    state: &AppState,
    request: Value,
    user_id: Option<Uuid>,
    knowledge_bases: Option<&[KnowledgeBase]>,
) -> Json<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": { "name": "GugleRAG", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "tools": {} }
        }),
        "tools/list" => json!({ "tools": mcp_tools() }),
        "tools/call" => match call_mcp_tool(state, params, user_id, knowledge_bases).await {
            Ok(value) => value,
            Err(error) => return Json(json_rpc_error(id, -32602, &error)),
        },
        _ => return Json(json_rpc_error(id, -32601, "method not found")),
    };
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

async fn knowledge_bases_for_token(
    state: &AppState,
    token: &crate::domain::McpToken,
) -> Result<Vec<KnowledgeBase>, AppError> {
    match token.scope.as_str() {
        "user" => {
            let (workspace, _) = state
                .database
                .ensure_personal_workspace(token.user_id)
                .await?;
            state.database.list_knowledge_bases(workspace.id).await
        }
        "group" => {
            let team_id = token
                .team_id
                .ok_or_else(|| AppError::Unauthorized("group token has no team".to_string()))?;
            if state
                .database
                .team_member_role(team_id, token.user_id)
                .await?
                .is_none()
            {
                return Err(AppError::Unauthorized(
                    "token owner is no longer a team member".to_string(),
                ));
            }
            let team = state
                .database
                .get_team(team_id)
                .await?
                .ok_or_else(|| AppError::NotFound("team not found".to_string()))?;
            state.database.list_knowledge_bases(team.workspace_id).await
        }
        "all" => {
            state
                .database
                .accessible_knowledge_bases(token.user_id)
                .await
        }
        _ => Err(AppError::Unauthorized("unknown MCP scope".to_string())),
    }
}

fn mcp_tools() -> Value {
    json!([
        {
            "name": "search_knowledge",
            "description": "Search the team knowledge base.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "read_document",
            "description": "Read a document by id.",
            "inputSchema": {
                "type": "object",
                "properties": { "doc_id": { "type": "string" } },
                "required": ["doc_id"]
            }
        },
        {
            "name": "create_document",
            "description": "Create a Markdown document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "parent_id": { "type": "string" },
                    "knowledge_base_id": { "type": "string" }
                },
                "required": ["title", "content"]
            }
        },
        {
            "name": "update_document",
            "description": "Update a document title/content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string" },
                    "title": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["doc_id"]
            }
        },
        {
            "name": "list_documents",
            "description": "List documents under a folder id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder_id": { "type": "string" },
                    "knowledge_base_id": { "type": "string" }
                }
            }
        },
        {
            "name": "get_document_metadata",
            "description": "Get document metadata without full content.",
            "inputSchema": {
                "type": "object",
                "properties": { "doc_id": { "type": "string" } },
                "required": ["doc_id"]
            }
        }
    ])
}

async fn call_mcp_tool(
    state: &AppState,
    params: Value,
    user_id: Option<Uuid>,
    allowed_knowledge_bases: Option<&[KnowledgeBase]>,
) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing tool name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let value = match name {
        "search_knowledge" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing query".to_string())?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
            let documents = documents_for_scope(state, allowed_knowledge_bases).await?;
            json!(search::search_documents(&documents, query, limit))
        }
        "read_document" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "document not found".to_string())?;
            require_document_scope(&doc, allowed_knowledge_bases)?;
            json!(doc)
        }
        "create_document" => {
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing title".to_string())?
                .trim()
                .to_string();
            if title.is_empty() {
                return Err("title cannot be empty".to_string());
            }
            let author_id = match user_id {
                Some(user_id) => user_id,
                None => system_user(&state.database)
                    .await
                    .map_err(|e| e.to_string())?,
            };
            let knowledge_base_id =
                resolve_knowledge_base_id(state, &args, author_id, allowed_knowledge_bases).await?;
            let parent_id = optional_uuid_arg(&args, "parent_id")?;
            if let Some(parent_id) = parent_id {
                let parent = state
                    .database
                    .get_document(parent_id)
                    .await
                    .map_err(|e| e.to_string())?;
                if !matches!(parent, Some(ref doc) if doc.knowledge_base_id == knowledge_base_id) {
                    return Err("parent_id must exist in the same knowledge base".to_string());
                }
            }
            let now = Utc::now();
            let doc = Document {
                id: Uuid::new_v4(),
                knowledge_base_id,
                title,
                content: args
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                parent_id,
                tags: Vec::new(),
                author_id,
                created_at: now,
                updated_at: now,
                versions: Vec::new(),
            };
            state
                .database
                .insert_document(&doc)
                .await
                .map_err(|e| e.to_string())?;
            json!(doc)
        }
        "update_document" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let mut doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "document not found".to_string())?;
            require_document_scope(&doc, allowed_knowledge_bases)?;
            if let Some(title) = args.get("title").and_then(Value::as_str) {
                if title.trim().is_empty() {
                    return Err("title cannot be empty".to_string());
                }
                doc.title = title.trim().to_string();
            }
            if let Some(content) = args.get("content").and_then(Value::as_str) {
                let version = DocumentVersion {
                    content: doc.content.clone(),
                    saved_at: Utc::now(),
                };
                state
                    .database
                    .insert_document_version(doc_id, &version)
                    .await
                    .map_err(|e| e.to_string())?;
                doc.versions.push(version);
                doc.content = content.to_string();
            }
            doc.updated_at = Utc::now();
            state
                .database
                .update_document(&doc)
                .await
                .map_err(|e| e.to_string())?;
            json!(doc)
        }
        "list_documents" => {
            let folder_id = optional_uuid_arg(&args, "folder_id")?;
            let knowledge_base_id = if let Some(folder_id) = folder_id {
                let folder = state
                    .database
                    .get_document(folder_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "folder_id not found".to_string())?;
                require_document_scope(&folder, allowed_knowledge_bases)?;
                folder.knowledge_base_id
            } else {
                let owner_id = match user_id {
                    Some(user_id) => user_id,
                    None => system_user(&state.database)
                        .await
                        .map_err(|e| e.to_string())?,
                };
                resolve_knowledge_base_id(state, &args, owner_id, allowed_knowledge_bases).await?
            };
            let docs = state
                .database
                .list_documents(folder_id, knowledge_base_id)
                .await
                .map_err(|e| e.to_string())?;
            json!(docs.iter().map(document_metadata).collect::<Vec<_>>())
        }
        "get_document_metadata" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "document not found".to_string())?;
            require_document_scope(&doc, allowed_knowledge_bases)?;
            document_metadata(&doc)
        }
        _ => return Err(format!("unknown tool: {name}")),
    };
    Ok(json!({ "content": [{ "type": "text", "text": value.to_string() }] }))
}

async fn documents_for_scope(
    state: &AppState,
    allowed_knowledge_bases: Option<&[KnowledgeBase]>,
) -> Result<Vec<Document>, String> {
    let Some(knowledge_bases) = allowed_knowledge_bases else {
        return state
            .database
            .all_documents()
            .await
            .map_err(|e| e.to_string());
    };
    let mut documents = Vec::new();
    for knowledge_base in knowledge_bases {
        documents.extend(
            state
                .database
                .all_documents_for_knowledge_base(knowledge_base.id)
                .await
                .map_err(|e| e.to_string())?,
        );
    }
    Ok(documents)
}

fn require_document_scope(
    document: &Document,
    allowed_knowledge_bases: Option<&[KnowledgeBase]>,
) -> Result<(), String> {
    if allowed_knowledge_bases.is_some_and(|knowledge_bases| {
        !knowledge_bases
            .iter()
            .any(|knowledge_base| knowledge_base.id == document.knowledge_base_id)
    }) {
        return Err("document is outside this MCP scope".to_string());
    }
    Ok(())
}

async fn resolve_knowledge_base_id(
    state: &AppState,
    args: &Value,
    user_id: Uuid,
    allowed_knowledge_bases: Option<&[KnowledgeBase]>,
) -> Result<Uuid, String> {
    let requested = optional_uuid_arg(args, "knowledge_base_id")?;
    if let Some(knowledge_bases) = allowed_knowledge_bases {
        if let Some(requested) = requested {
            return knowledge_bases
                .iter()
                .any(|knowledge_base| knowledge_base.id == requested)
                .then_some(requested)
                .ok_or_else(|| "knowledge_base_id is outside this MCP scope".to_string());
        }
        return match knowledge_bases {
            [knowledge_base] => Ok(knowledge_base.id),
            [] => Err("this MCP scope has no knowledge bases".to_string()),
            _ => Err(
                "knowledge_base_id is required when the scope has multiple knowledge bases"
                    .to_string(),
            ),
        };
    }
    if let Some(requested) = requested {
        return Ok(requested);
    }
    state
        .database
        .ensure_personal_workspace(user_id)
        .await
        .map(|(_, knowledge_base)| knowledge_base.id)
        .map_err(|error| error.to_string())
}

fn document_metadata(doc: &Document) -> Value {
    json!({
        "id": doc.id,
        "knowledge_base_id": doc.knowledge_base_id,
        "title": doc.title,
        "parent_id": doc.parent_id,
        "tags": doc.tags,
        "author_id": doc.author_id,
        "created_at": doc.created_at,
        "updated_at": doc.updated_at,
        "version_count": doc.versions.len()
    })
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn parse_uuid_arg(args: &Value, name: &str) -> Result<Uuid, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {name}"))?
        .parse()
        .map_err(|_| format!("{name} must be a uuid"))
}

fn optional_uuid_arg(args: &Value, name: &str) -> Result<Option<Uuid>, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.parse().map_err(|_| format!("{name} must be a uuid")))
        .transpose()
}

async fn system_user(database: &crate::db::Database) -> Result<Uuid, AppError> {
    if let Some(user) = database.find_user_by_username("mcp-system").await? {
        return Ok(user.id);
    }
    let id = Uuid::new_v4();
    let salt = Uuid::new_v4().to_string();
    database
        .insert_user(&User {
            id,
            username: "mcp-system".to_string(),
            display_name: "MCP System".to_string(),
            password_hash: hash_password(&salt, &Uuid::new_v4().to_string()),
            salt,
            role: Role::Admin,
            created_at: Utc::now(),
        })
        .await?;
    Ok(id)
}
