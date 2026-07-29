use crate::{
    AppState,
    auth::{self, hash_password},
    domain::{Document, DocumentVersion, Role, User},
    error::AppError,
    search,
};
use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/mcp", post(mcp_endpoint))
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
    if state.config.mcp_auth_required && auth::require_user(&headers, &state).await.is_err() {
        return Json(json_rpc_error(id, -32001, "authentication required"));
    }
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
        "tools/call" => match call_mcp_tool(&state, params).await {
            Ok(value) => value,
            Err(error) => return Json(json_rpc_error(id, -32602, &error)),
        },
        _ => return Json(json_rpc_error(id, -32601, "method not found")),
    };
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
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
                    "parent_id": { "type": "string" }
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
                "properties": { "folder_id": { "type": "string" } }
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

async fn call_mcp_tool(state: &AppState, params: Value) -> Result<Value, String> {
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
            let documents = state
                .database
                .all_documents()
                .await
                .map_err(|e| e.to_string())?;
            json!(search::search_documents(&documents, query, limit))
        }
        "read_document" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            json!(
                state
                    .database
                    .get_document(doc_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "document not found".to_string())?
            )
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
            let parent_id = optional_uuid_arg(&args, "parent_id")?;
            if let Some(parent_id) = parent_id {
                if !state
                    .database
                    .document_exists(parent_id)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    return Err("parent_id does not exist".to_string());
                }
            }
            let author_id = system_user(&state.database)
                .await
                .map_err(|e| e.to_string())?;
            let now = Utc::now();
            let doc = Document {
                id: Uuid::new_v4(),
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
            let docs = state
                .database
                .list_documents(folder_id)
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
            document_metadata(&doc)
        }
        _ => return Err(format!("unknown tool: {name}")),
    };
    Ok(json!({ "content": [{ "type": "text", "text": value.to_string() }] }))
}

fn document_metadata(doc: &Document) -> Value {
    json!({
        "id": doc.id,
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
