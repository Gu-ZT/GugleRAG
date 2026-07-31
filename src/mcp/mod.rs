use crate::{
    AppState,
    auth::{self, hash_password},
    domain::{Document, DocumentVersion, KnowledgeBase, Role, User, Workspace, WorkspaceKind},
    error::AppError,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

struct McpScope {
    workspaces: Vec<Workspace>,
    knowledge_bases: Vec<KnowledgeBase>,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/mcp", post(mcp_endpoint))
        .route("/mcp/all", post(all_workspaces_mcp_endpoint))
        .route("/mcp/{scope}/{workspace_id}", post(workspace_mcp_endpoint))
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
    let scope = match user_id {
        Some(user_id) => match mcp_scope_for_user(&state, user_id).await {
            Ok(scope) => Some(scope),
            Err(error) => return Json(json_rpc_error(id, -32000, &error.to_string())),
        },
        None => None,
    };
    mcp_response(&state, request, user_id, scope.as_ref()).await
}

async fn all_workspaces_mcp_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !state.config.mcp_enabled {
        return Json(json_rpc_error(id, -32000, "MCP endpoint disabled"));
    }
    let user_id = match auth::require_user(&headers, &state).await {
        Ok(user_id) => user_id,
        Err(error) => return Json(json_rpc_error(id, -32001, &error.to_string())),
    };
    let scope = match mcp_scope_for_user(&state, user_id).await {
        Ok(scope) => scope,
        Err(error) => return Json(json_rpc_error(id, -32001, &error.to_string())),
    };
    mcp_response(&state, request, Some(user_id), Some(&scope)).await
}

async fn workspace_mcp_endpoint(
    State(state): State<AppState>,
    Path((scope_name, workspace_id)): Path<(String, Uuid)>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !state.config.mcp_enabled {
        return Json(json_rpc_error(id, -32000, "MCP endpoint disabled"));
    }
    let user_id = match auth::require_user(&headers, &state).await {
        Ok(user_id) => user_id,
        Err(error) => return Json(json_rpc_error(id, -32001, &error.to_string())),
    };
    let scope = match mcp_scope_for_workspace(&state, user_id, &scope_name, workspace_id).await {
        Ok(scope) => scope,
        Err(error) => return Json(json_rpc_error(id, -32001, &error.to_string())),
    };
    mcp_response(&state, request, Some(user_id), Some(&scope)).await
}

async fn mcp_response(
    state: &AppState,
    request: Value,
    user_id: Option<Uuid>,
    scope: Option<&McpScope>,
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
        "tools/call" => match call_mcp_tool(state, params, user_id, scope).await {
            Ok(value) => value,
            Err(error) => return Json(json_rpc_error(id, -32602, &error)),
        },
        _ => return Json(json_rpc_error(id, -32601, "method not found")),
    };
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

async fn mcp_scope_for_user(state: &AppState, user_id: Uuid) -> Result<McpScope, AppError> {
    state.database.ensure_personal_workspace(user_id).await?;
    Ok(McpScope {
        workspaces: state.database.list_workspaces(user_id).await?,
        knowledge_bases: state.database.accessible_knowledge_bases(user_id).await?,
    })
}

async fn mcp_scope_for_workspace(
    state: &AppState,
    user_id: Uuid,
    scope_name: &str,
    workspace_id: Uuid,
) -> Result<McpScope, AppError> {
    if !state
        .database
        .user_can_access_workspace(user_id, workspace_id)
        .await?
    {
        return Err(AppError::Forbidden("workspace access denied".to_string()));
    }
    let workspace = state
        .database
        .get_workspace(workspace_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".to_string()))?;
    let valid_scope = matches!(
        (scope_name, workspace.kind),
        ("user", WorkspaceKind::Personal) | ("group", WorkspaceKind::Team)
    );
    if !valid_scope {
        return Err(AppError::BadRequest(
            "MCP scope does not match workspace kind".to_string(),
        ));
    }
    Ok(McpScope {
        knowledge_bases: state.database.list_knowledge_bases(workspace.id).await?,
        workspaces: vec![workspace],
    })
}

fn mcp_tools() -> Value {
    json!([
        {
            "name": "list_workspaces",
            "description": "List workspaces available in this MCP scope.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "list_knowledge_bases",
            "description": "List knowledge bases in an available workspace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string", "format": "uuid" }
                },
                "required": ["workspace_id"]
            }
        },
        {
            "name": "search_knowledge",
            "description": "Search documents across accessible workspaces and knowledge bases.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace_id": uuid_or_array_schema(
                        "Optional. Omit to search every workspace in this MCP scope."
                    ),
                    "knowledge_base_id": uuid_or_array_schema(
                        "Optional. Omit to search every knowledge base in the selected workspaces."
                    ),
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "read_document",
            "description": "Read a document in one workspace knowledge base.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string", "format": "uuid" },
                    "knowledge_base_id": { "type": "string", "format": "uuid" },
                    "doc_id": { "type": "string", "format": "uuid" }
                },
                "required": ["workspace_id", "knowledge_base_id", "doc_id"]
            }
        },
        {
            "name": "create_document",
            "description": "Create a Markdown document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string", "format": "uuid" },
                    "knowledge_base_id": { "type": "string", "format": "uuid" },
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "parent_id": { "type": "string", "format": "uuid" }
                },
                "required": ["workspace_id", "knowledge_base_id", "title", "content"]
            }
        },
        {
            "name": "update_document",
            "description": "Update a document title/content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string", "format": "uuid" },
                    "knowledge_base_id": { "type": "string", "format": "uuid" },
                    "doc_id": { "type": "string", "format": "uuid" },
                    "title": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["workspace_id", "knowledge_base_id", "doc_id"]
            }
        },
        {
            "name": "list_documents",
            "description": "List documents under a folder id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string", "format": "uuid" },
                    "knowledge_base_id": { "type": "string", "format": "uuid" },
                    "folder_id": { "type": "string", "format": "uuid" }
                },
                "required": ["workspace_id", "knowledge_base_id"]
            }
        },
        {
            "name": "get_document_metadata",
            "description": "Get document metadata without full content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string", "format": "uuid" },
                    "knowledge_base_id": { "type": "string", "format": "uuid" },
                    "doc_id": { "type": "string", "format": "uuid" }
                },
                "required": ["workspace_id", "knowledge_base_id", "doc_id"]
            }
        }
    ])
}

async fn call_mcp_tool(
    state: &AppState,
    params: Value,
    user_id: Option<Uuid>,
    scope: Option<&McpScope>,
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
        "list_workspaces" => match scope {
            Some(scope) => json!(scope.workspaces),
            None => json!(
                state
                    .database
                    .all_workspaces()
                    .await
                    .map_err(|e| e.to_string())?
            ),
        },
        "list_knowledge_bases" => {
            let workspace_id = parse_uuid_arg(&args, "workspace_id")?;
            json!(knowledge_bases_for_workspace(state, workspace_id, scope).await?)
        }
        "search_knowledge" => {
            let knowledge_bases = knowledge_bases_for_search(state, &args, scope).await?;
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing query".to_string())?;
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .clamp(1, 50) as usize;
            let mut documents = Vec::new();
            let mut document_contexts = HashMap::new();
            for knowledge_base in &knowledge_bases {
                let knowledge_base_documents = state
                    .database
                    .all_documents_for_knowledge_base(knowledge_base.id)
                    .await
                    .map_err(|e| e.to_string())?;
                for document in &knowledge_base_documents {
                    document_contexts.insert(
                        document.id,
                        (knowledge_base.workspace_id, knowledge_base.id),
                    );
                }
                documents.extend(knowledge_base_documents);
            }
            let results = state
                .search
                .search_documents(&documents, query, limit)
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|result| {
                    let (workspace_id, knowledge_base_id) =
                        document_contexts.get(&result.id).copied().ok_or_else(|| {
                            "search result is missing its resource context".to_string()
                        })?;
                    Ok(json!({
                        "id": result.id,
                        "workspace_id": workspace_id,
                        "knowledge_base_id": knowledge_base_id,
                        "title": result.title,
                        "excerpt": result.excerpt,
                        "score": result.score,
                        "updated_at": result.updated_at
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
            json!(results)
        }
        "read_document" => {
            let (workspace_id, knowledge_base_id) =
                require_document_context(state, &args, scope).await?;
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "document not found".to_string())?;
            require_document_knowledge_base(&doc, knowledge_base_id)?;
            document_value(&doc, workspace_id)
        }
        "create_document" => {
            let (workspace_id, knowledge_base_id) =
                require_document_context(state, &args, scope).await?;
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
            let parent_id = optional_uuid_arg(&args, "parent_id")?;
            if let Some(parent_id) = parent_id {
                let parent = state
                    .database
                    .get_document(parent_id)
                    .await
                    .map_err(|e| e.to_string())?;
                if !matches!(parent, Some(ref doc) if doc.knowledge_base_id == knowledge_base_id && doc.is_folder)
                {
                    return Err(
                        "parent_id must be an existing folder in the same knowledge base"
                            .to_string(),
                    );
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
                is_folder: false,
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
            document_value(&doc, workspace_id)
        }
        "update_document" => {
            let (workspace_id, knowledge_base_id) =
                require_document_context(state, &args, scope).await?;
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let mut doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "document not found".to_string())?;
            require_document_knowledge_base(&doc, knowledge_base_id)?;
            if let Some(title) = args.get("title").and_then(Value::as_str) {
                if title.trim().is_empty() {
                    return Err("title cannot be empty".to_string());
                }
                doc.title = title.trim().to_string();
            }
            if let Some(content) = args.get("content").and_then(Value::as_str) {
                if doc.is_folder {
                    return Err("folders cannot have content".to_string());
                }
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
            document_value(&doc, workspace_id)
        }
        "list_documents" => {
            let (workspace_id, knowledge_base_id) =
                require_document_context(state, &args, scope).await?;
            let folder_id = optional_uuid_arg(&args, "folder_id")?;
            if let Some(folder_id) = folder_id {
                let folder = state
                    .database
                    .get_document(folder_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "folder_id not found".to_string())?;
                require_document_knowledge_base(&folder, knowledge_base_id)?;
                if !folder.is_folder {
                    return Err("folder_id must refer to a folder".to_string());
                }
            }
            let docs = state
                .database
                .list_documents(folder_id, knowledge_base_id)
                .await
                .map_err(|e| e.to_string())?;
            json!(
                docs.iter()
                    .map(|doc| document_metadata(doc, workspace_id))
                    .collect::<Vec<_>>()
            )
        }
        "get_document_metadata" => {
            let (workspace_id, knowledge_base_id) =
                require_document_context(state, &args, scope).await?;
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "document not found".to_string())?;
            require_document_knowledge_base(&doc, knowledge_base_id)?;
            document_metadata(&doc, workspace_id)
        }
        _ => return Err(format!("unknown tool: {name}")),
    };
    Ok(json!({ "content": [{ "type": "text", "text": value.to_string() }] }))
}

async fn knowledge_bases_for_workspace(
    state: &AppState,
    workspace_id: Uuid,
    scope: Option<&McpScope>,
) -> Result<Vec<KnowledgeBase>, String> {
    if let Some(scope) = scope {
        if !scope
            .workspaces
            .iter()
            .any(|workspace| workspace.id == workspace_id)
        {
            return Err("workspace_id is outside this MCP scope".to_string());
        }
        return Ok(scope
            .knowledge_bases
            .iter()
            .filter(|knowledge_base| knowledge_base.workspace_id == workspace_id)
            .cloned()
            .collect());
    }
    state
        .database
        .get_workspace(workspace_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workspace_id not found".to_string())?;
    state
        .database
        .list_knowledge_bases(workspace_id)
        .await
        .map_err(|e| e.to_string())
}

async fn knowledge_bases_for_search(
    state: &AppState,
    args: &Value,
    scope: Option<&McpScope>,
) -> Result<Vec<KnowledgeBase>, String> {
    let workspace_ids = optional_uuid_array_arg(args, "workspace_id")?;
    let knowledge_base_ids = optional_uuid_array_arg(args, "knowledge_base_id")?;
    let workspaces = workspaces_for_search(state, scope, workspace_ids.as_deref()).await?;
    let mut available_knowledge_bases = Vec::new();
    for workspace in workspaces {
        available_knowledge_bases
            .extend(knowledge_bases_for_workspace(state, workspace.id, scope).await?);
    }

    let Some(knowledge_base_ids) = knowledge_base_ids else {
        return Ok(available_knowledge_bases);
    };
    knowledge_base_ids
        .into_iter()
        .map(|knowledge_base_id| {
            available_knowledge_bases
                .iter()
                .find(|knowledge_base| knowledge_base.id == knowledge_base_id)
                .cloned()
                .ok_or_else(|| {
                    "knowledge_base_id is outside the selected workspaces or MCP scope".to_string()
                })
        })
        .collect()
}

async fn workspaces_for_search(
    state: &AppState,
    scope: Option<&McpScope>,
    workspace_ids: Option<&[Uuid]>,
) -> Result<Vec<Workspace>, String> {
    let available_workspaces = match scope {
        Some(scope) => scope.workspaces.clone(),
        None => state
            .database
            .all_workspaces()
            .await
            .map_err(|e| e.to_string())?,
    };
    let Some(workspace_ids) = workspace_ids else {
        return Ok(available_workspaces);
    };
    workspace_ids
        .iter()
        .map(|workspace_id| {
            available_workspaces
                .iter()
                .find(|workspace| workspace.id == *workspace_id)
                .cloned()
                .ok_or_else(|| "workspace_id is outside this MCP scope".to_string())
        })
        .collect()
}

async fn require_document_context(
    state: &AppState,
    args: &Value,
    scope: Option<&McpScope>,
) -> Result<(Uuid, Uuid), String> {
    let workspace_id = parse_uuid_arg(args, "workspace_id")?;
    let knowledge_base_id = parse_uuid_arg(args, "knowledge_base_id")?;
    let knowledge_bases = knowledge_bases_for_workspace(state, workspace_id, scope).await?;
    knowledge_bases
        .iter()
        .any(|knowledge_base| knowledge_base.id == knowledge_base_id)
        .then_some((workspace_id, knowledge_base_id))
        .ok_or_else(|| "knowledge_base_id does not belong to workspace_id or MCP scope".to_string())
}

fn require_document_knowledge_base(
    document: &Document,
    knowledge_base_id: Uuid,
) -> Result<(), String> {
    if document.knowledge_base_id != knowledge_base_id {
        return Err("document does not belong to knowledge_base_id".to_string());
    }
    Ok(())
}

fn document_value(doc: &Document, workspace_id: Uuid) -> Value {
    let mut value = json!(doc);
    value["workspace_id"] = json!(workspace_id);
    value
}

fn document_metadata(doc: &Document, workspace_id: Uuid) -> Value {
    json!({
        "id": doc.id,
        "workspace_id": workspace_id,
        "knowledge_base_id": doc.knowledge_base_id,
        "title": doc.title,
        "parent_id": doc.parent_id,
        "is_folder": doc.is_folder,
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

fn optional_uuid_array_arg(args: &Value, name: &str) -> Result<Option<Vec<Uuid>>, String> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    let values = match value {
        Value::String(value) => vec![value.as_str()],
        Value::Array(values) => {
            if values.is_empty() {
                return Err(format!("{name} must not be an empty array"));
            }
            values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| format!("{name} must be a uuid or an array of uuids"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(format!("{name} must be a uuid or an array of uuids")),
    };
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let id = value
            .parse()
            .map_err(|_| format!("{name} must contain only uuids"))?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    Ok(Some(ids))
}

fn uuid_or_array_schema(description: &str) -> Value {
    json!({
        "description": description,
        "oneOf": [
            { "type": "string", "format": "uuid" },
            {
                "type": "array",
                "minItems": 1,
                "items": { "type": "string", "format": "uuid" }
            }
        ]
    })
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
