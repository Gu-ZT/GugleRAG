use super::collaboration;
use crate::{
    AppState, auth,
    auth::can_edit,
    domain::{Document, DocumentVersion},
    error::AppError,
};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;
use zip::ZipArchive;

pub(crate) const MAX_ZIP_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_ZIP_MULTIPART_BYTES: usize = MAX_ZIP_UPLOAD_BYTES + 64 * 1024;
const MAX_ZIP_ENTRIES: usize = 1_000;
const MAX_TEXT_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_EXTRACTED_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PATH_DEPTH: usize = 64;
const MAX_PATH_COMPONENT_CHARS: usize = 255;
const MAX_SKIP_DETAILS: usize = 50;

#[derive(Debug, Deserialize)]
pub(crate) struct DocumentRequest {
    pub(crate) title: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) parent_id: Option<Uuid>,
    pub(crate) is_folder: Option<bool>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) knowledge_base_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    pub(crate) parent_id: Option<Uuid>,
    pub(crate) knowledge_base_id: Option<Uuid>,
    #[serde(default)]
    pub(crate) tree: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ZipImportResult {
    imported_files: usize,
    created_folders: usize,
    skipped_entries: usize,
    skips: Vec<ZipImportSkip>,
}

#[derive(Debug, Clone, Serialize)]
struct ZipImportSkip {
    path: String,
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ZipImportJobStatus {
    Queued,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
struct ZipImportJob {
    user_id: Uuid,
    knowledge_base_id: Uuid,
    status: ZipImportJobStatus,
    processed_entries: usize,
    total_entries: usize,
    result: Option<ZipImportResult>,
    error: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ZipImportStatusResponse {
    job_id: Uuid,
    status: ZipImportJobStatus,
    processed_entries: usize,
    total_entries: usize,
    progress: u8,
    imported_files: usize,
    created_folders: usize,
    skipped_entries: usize,
    skips: Vec<ZipImportSkip>,
    error: Option<String>,
}

struct ZipImportEntry {
    path: String,
    components: Vec<String>,
    content: Option<String>,
}

type DocumentLocation = (Option<Uuid>, String);

static ZIP_IMPORT_JOBS: OnceLock<Mutex<HashMap<Uuid, ZipImportJob>>> = OnceLock::new();

fn zip_import_jobs() -> &'static Mutex<HashMap<Uuid, ZipImportJob>> {
    ZIP_IMPORT_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_zip_import(user_id: Uuid, knowledge_base_id: Uuid) -> Uuid {
    let job_id = Uuid::new_v4();
    let mut jobs = zip_import_jobs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cutoff = Utc::now() - Duration::hours(1);
    jobs.retain(|_, job| job.created_at > cutoff);
    jobs.insert(
        job_id,
        ZipImportJob {
            user_id,
            knowledge_base_id,
            status: ZipImportJobStatus::Queued,
            processed_entries: 0,
            total_entries: 0,
            result: None,
            error: None,
            created_at: Utc::now(),
        },
    );
    job_id
}

fn update_zip_import_progress(job_id: Uuid, processed_entries: usize, total_entries: usize) {
    let mut jobs = zip_import_jobs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(job) = jobs.get_mut(&job_id) {
        job.status = ZipImportJobStatus::Processing;
        job.processed_entries = processed_entries.min(total_entries);
        job.total_entries = total_entries;
    }
}

fn complete_zip_import(job_id: Uuid, result: ZipImportResult) {
    let mut jobs = zip_import_jobs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(job) = jobs.get_mut(&job_id) {
        job.status = ZipImportJobStatus::Completed;
        job.processed_entries = job.total_entries;
        job.result = Some(result);
        job.error = None;
    }
}

fn fail_zip_import(job_id: Uuid, error: String) {
    let mut jobs = zip_import_jobs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(job) = jobs.get_mut(&job_id) {
        job.status = ZipImportJobStatus::Failed;
        job.error = Some(error);
    }
}

fn zip_import_scope(job_id: Uuid) -> Option<(Uuid, Uuid)> {
    zip_import_jobs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&job_id)
        .map(|job| (job.user_id, job.knowledge_base_id))
}

fn zip_import_status_response(job_id: Uuid) -> Option<ZipImportStatusResponse> {
    let jobs = zip_import_jobs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = jobs.get(&job_id)?;
    let progress = if job.status == ZipImportJobStatus::Completed {
        100
    } else if job.total_entries == 0 {
        0
    } else {
        job.processed_entries
            .saturating_mul(100)
            .checked_div(job.total_entries)
            .unwrap_or_default()
            .min(99) as u8
    };
    let result = job.result.as_ref();
    Some(ZipImportStatusResponse {
        job_id,
        status: job.status,
        processed_entries: job.processed_entries,
        total_entries: job.total_entries,
        progress,
        imported_files: result.map_or(0, |result| result.imported_files),
        created_folders: result.map_or(0, |result| result.created_folders),
        skipped_entries: result.map_or(0, |result| result.skipped_entries),
        skips: result.map_or_else(Vec::new, |result| result.skips.clone()),
        error: job.error.clone(),
    })
}

pub(crate) async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Document>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let knowledge_base = resolve_knowledge_base(&state, user_id, query.knowledge_base_id).await?;
    if !query.tree
        && let Some(parent_id) = query.parent_id
    {
        require_parent_in_knowledge_base(&state, parent_id, knowledge_base.id).await?;
    }
    let documents = if query.tree {
        state
            .database
            .all_documents_for_knowledge_base(knowledge_base.id)
            .await?
    } else {
        state
            .database
            .list_documents(query.parent_id, knowledge_base.id)
            .await?
    };
    Ok(Json(documents))
}

pub(crate) async fn create_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DocumentRequest>,
) -> Result<Json<Document>, AppError> {
    let user_id = require_editor(&state, &headers).await?;
    let knowledge_base = resolve_knowledge_base(&state, user_id, input.knowledge_base_id).await?;
    let is_folder = input.is_folder.unwrap_or(false);
    if is_folder
        && (input
            .content
            .as_deref()
            .is_some_and(|content| !content.trim().is_empty())
            || input.tags.as_ref().is_some_and(|tags| !tags.is_empty()))
    {
        return Err(AppError::BadRequest(
            "folders cannot have content or tags".to_string(),
        ));
    }
    let title = auth::require_non_empty(input.title, "title")?;
    let now = Utc::now();
    let doc = Document {
        id: Uuid::new_v4(),
        knowledge_base_id: knowledge_base.id,
        title,
        content: if is_folder {
            String::new()
        } else {
            input.content.unwrap_or_default()
        },
        parent_id: input.parent_id,
        is_folder,
        tags: if is_folder {
            Vec::new()
        } else {
            input.tags.unwrap_or_default()
        },
        author_id: user_id,
        created_at: now,
        updated_at: now,
        versions: Vec::new(),
    };
    if let Some(parent_id) = doc.parent_id {
        require_parent_in_knowledge_base(&state, parent_id, doc.knowledge_base_id).await?;
    }
    state.database.insert_document(&doc).await?;
    Ok(Json(doc))
}

pub(crate) async fn import_zip(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(knowledge_base_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ZipImportStatusResponse>), AppError> {
    let user_id = require_editor(&state, &headers).await?;
    collaboration::require_knowledge_base_access(&state, user_id, knowledge_base_id).await?;
    let archive_bytes = read_zip_upload(&mut multipart).await?;
    let job_id = register_zip_import(user_id, knowledge_base_id);
    let task_state = state.clone();
    tokio::spawn(async move {
        match process_zip_import(
            &task_state,
            user_id,
            knowledge_base_id,
            archive_bytes,
            job_id,
        )
        .await
        {
            Ok(result) => complete_zip_import(job_id, result),
            Err(error) => fail_zip_import(job_id, error.to_string()),
        }
    });
    let status = zip_import_status_response(job_id)
        .ok_or_else(|| AppError::Internal("failed to create ZIP import job".to_string()))?;
    Ok((StatusCode::ACCEPTED, Json(status)))
}

pub(crate) async fn zip_import_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((knowledge_base_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ZipImportStatusResponse>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    collaboration::require_knowledge_base_access(&state, user_id, knowledge_base_id).await?;
    let Some((owner_id, job_knowledge_base_id)) = zip_import_scope(job_id) else {
        return Err(AppError::NotFound("ZIP import job not found".to_string()));
    };
    if owner_id != user_id || job_knowledge_base_id != knowledge_base_id {
        return Err(AppError::NotFound("ZIP import job not found".to_string()));
    }
    let status = zip_import_status_response(job_id)
        .ok_or_else(|| AppError::NotFound("ZIP import job not found".to_string()))?;
    Ok(Json(status))
}

async fn process_zip_import(
    state: &AppState,
    user_id: Uuid,
    knowledge_base_id: Uuid,
    archive_bytes: Vec<u8>,
    job_id: Uuid,
) -> Result<ZipImportResult, AppError> {
    let (mut result, entries, archive_entry_count) = {
        let mut archive = ZipArchive::new(Cursor::new(archive_bytes))
            .map_err(|error| AppError::BadRequest(format!("invalid ZIP archive: {error}")))?;
        if archive.len() > MAX_ZIP_ENTRIES {
            return Err(AppError::BadRequest(format!(
                "ZIP archive contains more than {MAX_ZIP_ENTRIES} entries"
            )));
        }
        let archive_entry_count = archive.len();
        update_zip_import_progress(job_id, 0, archive_entry_count);
        let mut result = ZipImportResult {
            imported_files: 0,
            created_folders: 0,
            skipped_entries: 0,
            skips: Vec::new(),
        };
        let entries = collect_zip_entries(&mut archive, &mut result, job_id);
        (result, entries, archive_entry_count)
    };
    let total_entries = archive_entry_count.saturating_add(entries.len());
    update_zip_import_progress(job_id, archive_entry_count, total_entries);
    let existing_documents = state
        .database
        .all_documents_for_knowledge_base(knowledge_base_id)
        .await?;
    let mut documents_by_location = existing_documents
        .into_iter()
        .map(|document| ((document.parent_id, document.title.clone()), document))
        .collect::<HashMap<DocumentLocation, Document>>();
    for (index, entry) in entries.into_iter().enumerate() {
        import_zip_entry(
            state,
            &mut documents_by_location,
            &mut result,
            knowledge_base_id,
            user_id,
            entry,
        )
        .await?;
        update_zip_import_progress(
            job_id,
            archive_entry_count.saturating_add(index + 1),
            total_entries,
        );
    }

    Ok(result)
}

async fn import_zip_entry(
    state: &AppState,
    documents_by_location: &mut HashMap<DocumentLocation, Document>,
    result: &mut ZipImportResult,
    knowledge_base_id: Uuid,
    user_id: Uuid,
    entry: ZipImportEntry,
) -> Result<(), AppError> {
    let ZipImportEntry {
        path,
        components,
        content,
    } = entry;
    let Some(content) = content else {
        if components.is_empty() {
            return Ok(());
        }
        if let Err(error) = ensure_folder_path(
            state,
            documents_by_location,
            result,
            knowledge_base_id,
            user_id,
            &components,
        )
        .await
        {
            record_skip(result, path, error.to_string());
        }
        return Ok(());
    };

    let (file_name, parent_components) = components
        .split_last()
        .expect("non-empty ZIP path components");
    let parent_id = match ensure_folder_path(
        state,
        documents_by_location,
        result,
        knowledge_base_id,
        user_id,
        parent_components,
    )
    .await
    {
        Ok(parent_id) => parent_id,
        Err(error) => {
            record_skip(result, path, error.to_string());
            return Ok(());
        }
    };
    let location = (parent_id, file_name.clone());
    if documents_by_location.contains_key(&location) {
        record_skip(result, path, "an item with the same name already exists");
        return Ok(());
    }
    let now = Utc::now();
    let document = Document {
        id: Uuid::new_v4(),
        knowledge_base_id,
        title: file_name.clone(),
        content,
        parent_id,
        is_folder: false,
        tags: Vec::new(),
        author_id: user_id,
        created_at: now,
        updated_at: now,
        versions: Vec::new(),
    };
    state.database.insert_document(&document).await?;
    documents_by_location.insert(location, document);
    result.imported_files += 1;
    Ok(())
}

pub(crate) async fn read_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Document>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    collaboration::require_knowledge_base_access(&state, user_id, doc.knowledge_base_id).await?;
    Ok(Json(doc))
}

pub(crate) async fn update_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<DocumentRequest>,
) -> Result<Json<Document>, AppError> {
    let user_id = require_editor(&state, &headers).await?;
    let mut doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    collaboration::require_knowledge_base_access(&state, user_id, doc.knowledge_base_id).await?;
    if input
        .knowledge_base_id
        .is_some_and(|knowledge_base_id| knowledge_base_id != doc.knowledge_base_id)
    {
        return Err(AppError::BadRequest(
            "moving documents between knowledge bases is not supported".to_string(),
        ));
    }
    if input
        .is_folder
        .is_some_and(|is_folder| is_folder != doc.is_folder)
    {
        return Err(AppError::BadRequest(
            "changing an item's folder type is not supported".to_string(),
        ));
    }
    if doc.is_folder && (input.content.is_some() || input.tags.is_some()) {
        return Err(AppError::BadRequest(
            "folders cannot have content or tags".to_string(),
        ));
    }
    if let Some(parent_id) = input.parent_id {
        if parent_id == id {
            return Err(AppError::BadRequest(
                "document cannot be its own parent".to_string(),
            ));
        }
        require_parent_in_knowledge_base(&state, parent_id, doc.knowledge_base_id).await?;
        if state.database.document_is_descendant(parent_id, id).await? {
            return Err(AppError::BadRequest(
                "document cannot be moved into one of its descendants".to_string(),
            ));
        }
    }
    if input.content.is_some() {
        let version = DocumentVersion {
            content: doc.content.clone(),
            saved_at: Utc::now(),
        };
        state.database.insert_document_version(id, &version).await?;
        doc.versions.push(version);
    }
    if let Some(title) = input.title {
        doc.title = auth::require_non_empty(Some(title), "title")?;
    }
    if let Some(content) = input.content {
        doc.content = content;
    }
    if input.parent_id.is_some() {
        doc.parent_id = input.parent_id;
    }
    if let Some(tags) = input.tags {
        doc.tags = tags;
    }
    doc.updated_at = Utc::now();
    state.database.update_document(&doc).await?;
    Ok(Json(doc))
}

pub(crate) async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_editor(&state, &headers).await?;
    let doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    collaboration::require_knowledge_base_access(&state, user_id, doc.knowledge_base_id).await?;
    state.database.delete_document_tree(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn require_editor(state: &AppState, headers: &HeaderMap) -> Result<Uuid, AppError> {
    let user_id = auth::require_user(headers, state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    if !can_edit(&user) {
        return Err(AppError::Forbidden("insufficient role".to_string()));
    }
    Ok(user_id)
}

async fn resolve_knowledge_base(
    state: &AppState,
    user_id: Uuid,
    knowledge_base_id: Option<Uuid>,
) -> Result<crate::domain::KnowledgeBase, AppError> {
    match knowledge_base_id {
        Some(id) => collaboration::require_knowledge_base_access(state, user_id, id).await,
        None => collaboration::default_knowledge_base(state, user_id).await,
    }
}

async fn require_parent_in_knowledge_base(
    state: &AppState,
    parent_id: Uuid,
    knowledge_base_id: Uuid,
) -> Result<(), AppError> {
    let parent = state.database.get_document(parent_id).await?;
    if !matches!(parent, Some(ref doc) if doc.knowledge_base_id == knowledge_base_id && doc.is_folder)
    {
        return Err(AppError::BadRequest(
            "parent_id must be an existing folder in the same knowledge base".to_string(),
        ));
    }
    Ok(())
}

async fn read_zip_upload(multipart: &mut Multipart) -> Result<Vec<u8>, AppError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::BadRequest(format!("invalid multipart upload: {error}")))?
    {
        let field_name = field.name().map(ToOwned::to_owned);
        if !matches!(field_name.as_deref(), Some("archive") | Some("file")) {
            continue;
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|error| AppError::BadRequest(format!("could not read ZIP upload: {error}")))?;
        if bytes.len() > MAX_ZIP_UPLOAD_BYTES {
            return Err(AppError::BadRequest(format!(
                "ZIP upload exceeds the {MAX_ZIP_UPLOAD_BYTES}-byte limit"
            )));
        }
        return Ok(bytes.to_vec());
    }
    Err(AppError::BadRequest(
        "multipart upload must include an archive field".to_string(),
    ))
}

fn collect_zip_entries(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    result: &mut ZipImportResult,
    job_id: Uuid,
) -> Vec<ZipImportEntry> {
    let mut entries = Vec::new();
    let mut extracted_bytes = 0u64;
    let total_entries = archive.len();
    for index in 0..total_entries {
        let entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => {
                record_skip(result, format!("entry-{index}"), error.to_string());
                update_zip_import_progress(job_id, index + 1, total_entries);
                continue;
            }
        };
        let path = entry.name().to_string();
        let components = match zip_path_components(&path) {
            Ok(components) => components,
            Err(reason) => {
                record_skip(result, path, reason);
                update_zip_import_progress(job_id, index + 1, total_entries);
                continue;
            }
        };
        if path.ends_with('/') {
            entries.push(ZipImportEntry {
                path,
                components,
                content: None,
            });
            update_zip_import_progress(job_id, index + 1, total_entries);
            continue;
        }
        if components.is_empty() {
            record_skip(result, path, "entry has no file name");
            update_zip_import_progress(job_id, index + 1, total_entries);
            continue;
        }
        let declared_size = entry.size();
        if declared_size > MAX_TEXT_FILE_BYTES {
            record_skip(
                result,
                path,
                format!("file exceeds the {MAX_TEXT_FILE_BYTES}-byte limit"),
            );
            update_zip_import_progress(job_id, index + 1, total_entries);
            continue;
        }
        if extracted_bytes.saturating_add(declared_size) > MAX_TOTAL_EXTRACTED_BYTES {
            record_skip(
                result,
                path,
                format!("archive exceeds the {MAX_TOTAL_EXTRACTED_BYTES}-byte extracted limit"),
            );
            update_zip_import_progress(job_id, index + 1, total_entries);
            continue;
        }
        let mut bytes = Vec::with_capacity(declared_size as usize);
        let mut limited_reader = entry.take(MAX_TEXT_FILE_BYTES + 1);
        if let Err(error) = limited_reader.read_to_end(&mut bytes) {
            record_skip(result, path, format!("could not read file: {error}"));
            update_zip_import_progress(job_id, index + 1, total_entries);
            continue;
        }
        let actual_size = bytes.len() as u64;
        if actual_size > MAX_TEXT_FILE_BYTES {
            record_skip(
                result,
                path,
                format!("file exceeds the {MAX_TEXT_FILE_BYTES}-byte limit"),
            );
            update_zip_import_progress(job_id, index + 1, total_entries);
            continue;
        }
        if extracted_bytes.saturating_add(actual_size) > MAX_TOTAL_EXTRACTED_BYTES {
            record_skip(
                result,
                path,
                format!("archive exceeds the {MAX_TOTAL_EXTRACTED_BYTES}-byte extracted limit"),
            );
            update_zip_import_progress(job_id, index + 1, total_entries);
            continue;
        }
        let content = match decode_text(bytes) {
            Ok(content) => content,
            Err(reason) => {
                record_skip(result, path, reason);
                update_zip_import_progress(job_id, index + 1, total_entries);
                continue;
            }
        };
        extracted_bytes += actual_size;
        entries.push(ZipImportEntry {
            path,
            components,
            content: Some(content),
        });
        update_zip_import_progress(job_id, index + 1, total_entries);
    }
    entries
}

async fn ensure_folder_path(
    state: &AppState,
    documents_by_location: &mut HashMap<DocumentLocation, Document>,
    result: &mut ZipImportResult,
    knowledge_base_id: Uuid,
    author_id: Uuid,
    components: &[String],
) -> Result<Option<Uuid>, AppError> {
    let mut parent_id = None;
    for component in components {
        let location = (parent_id, component.clone());
        if let Some(existing) = documents_by_location.get(&location) {
            if !existing.is_folder {
                return Err(AppError::Conflict(format!(
                    "{} conflicts with an existing document",
                    component
                )));
            }
            parent_id = Some(existing.id);
            continue;
        }
        let now = Utc::now();
        let folder = Document {
            id: Uuid::new_v4(),
            knowledge_base_id,
            title: component.clone(),
            content: String::new(),
            parent_id,
            is_folder: true,
            tags: Vec::new(),
            author_id,
            created_at: now,
            updated_at: now,
            versions: Vec::new(),
        };
        state.database.insert_document(&folder).await?;
        parent_id = Some(folder.id);
        documents_by_location.insert(location, folder);
        result.created_folders += 1;
    }
    Ok(parent_id)
}

fn zip_path_components(raw_path: &str) -> Result<Vec<String>, &'static str> {
    let normalized = raw_path.replace('\\', "/");
    if normalized.starts_with('/') || normalized.starts_with("//") {
        return Err("absolute paths are not allowed");
    }
    let mut components = normalized.split('/').collect::<Vec<_>>();
    if components.last() == Some(&"") {
        components.pop();
    }
    if components.len() > MAX_PATH_DEPTH {
        return Err("path is too deeply nested");
    }
    if components.iter().any(|component| {
        component.is_empty()
            || matches!(*component, "." | "..")
            || component.contains(':')
            || component.chars().count() > MAX_PATH_COMPONENT_CHARS
            || component.chars().any(char::is_control)
    }) {
        return Err("path contains an unsafe component");
    }
    Ok(components.into_iter().map(ToOwned::to_owned).collect())
}

fn decode_text(bytes: Vec<u8>) -> Result<String, &'static str> {
    if bytes.contains(&0) {
        return Err("binary files are not imported");
    }
    let text = String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8 text")?;
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("binary files are not imported");
    }
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

fn record_skip(result: &mut ZipImportResult, path: impl Into<String>, reason: impl Into<String>) {
    result.skipped_entries += 1;
    if result.skips.len() < MAX_SKIP_DETAILS {
        result.skips.push(ZipImportSkip {
            path: path.into(),
            reason: reason.into(),
        });
    }
}
