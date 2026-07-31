use crate::{config::validate_vector_database_url, embedding::cosine_similarity, error::AppError};
use instant_distance::{Builder, HnswMap, Point, Search};
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgPoolOptions};
use std::{
    collections::HashMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use tokio::sync::OnceCell;
use tracing::warn;
use uuid::Uuid;

const INDEX_FORMAT_VERSION: u32 = 1;
const INDEX_EF_SEARCH: usize = 256;
const INDEX_EF_CONSTRUCTION: usize = 256;
const POSTGRES_TABLE: &str = "guglerag_vector_embeddings";
const POSTGRES_HNSW_MAX_DIMENSIONS: usize = 2_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct VectorIndexEntry {
    pub(crate) document_id: Uuid,
    pub(crate) knowledge_base_id: Uuid,
    pub(crate) chunk_index: usize,
    pub(crate) content_hash: String,
    pub(crate) text: String,
}

#[derive(Clone)]
pub(crate) struct VectorIndexPoint {
    pub(crate) entry: VectorIndexEntry,
    pub(crate) vector: Vec<f32>,
}

#[derive(Clone)]
pub(crate) struct VectorSearchHit {
    pub(crate) entry: VectorIndexEntry,
    pub(crate) score: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EmbeddingPoint(Vec<f32>);

impl Point for EmbeddingPoint {
    fn distance(&self, other: &Self) -> f32 {
        1.0 - cosine_similarity(&self.0, &other.0)
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedVectorIndex {
    format_version: u32,
    knowledge_base_id: Uuid,
    provider: String,
    model: String,
    dimensions: usize,
    map: HnswMap<EmbeddingPoint, VectorIndexEntry>,
}

#[derive(Clone)]
pub(crate) struct VectorStore {
    directory: Option<Arc<PathBuf>>,
    indexes: Arc<RwLock<HashMap<Uuid, PersistedVectorIndex>>>,
    postgres: Option<sqlx::PgPool>,
    postgres_ready: Arc<OnceCell<()>>,
}

impl VectorStore {
    pub(crate) fn new(
        directory: PathBuf,
        vector_database_url: Option<&str>,
    ) -> Result<Self, AppError> {
        let vector_database_url = vector_database_url
            .map(str::trim)
            .filter(|url| !url.is_empty());
        let postgres = if let Some(url) = vector_database_url {
            validate_vector_database_url(url)?;
            Some(
                PgPoolOptions::new()
                    .max_connections(8)
                    .connect_lazy(url)
                    .map_err(|error| {
                        AppError::Internal(format!(
                            "failed to configure PostgreSQL vector database: {error}"
                        ))
                    })?,
            )
        } else {
            None
        };
        let directory = if postgres.is_none() {
            fs::create_dir_all(&directory).map_err(|error| {
                AppError::Internal(format!(
                    "failed to create vector index directory {}: {error}",
                    directory.display()
                ))
            })?;
            Some(Arc::new(directory))
        } else {
            None
        };
        Ok(Self {
            directory,
            indexes: Arc::new(RwLock::new(HashMap::new())),
            postgres,
            postgres_ready: Arc::new(OnceCell::new()),
        })
    }

    pub(crate) fn backend_name(&self) -> &'static str {
        if self.postgres.is_some() {
            "postgres-pgvector"
        } else {
            "embedded-hnsw"
        }
    }

    pub(crate) async fn current_points(
        &self,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        expected: &[VectorIndexEntry],
    ) -> Result<Option<Vec<VectorIndexPoint>>, AppError> {
        if self.postgres.is_some() {
            self.ensure_postgres_schema().await?;
            let pool = self.postgres_pool()?;
            return self
                .postgres_current_points(pool, knowledge_base_id, provider, model, expected)
                .await;
        }
        Ok(self.embedded_current_points(knowledge_base_id, provider, model, expected))
    }

    pub(crate) async fn replace_index(
        &self,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        points: Vec<VectorIndexPoint>,
    ) -> Result<(), AppError> {
        let dimensions = validate_points(knowledge_base_id, &points)?;
        if self.postgres.is_some() {
            self.ensure_postgres_schema().await?;
            let pool = self.postgres_pool()?;
            return self
                .postgres_replace_index(
                    pool,
                    knowledge_base_id,
                    provider,
                    model,
                    &points,
                    dimensions,
                )
                .await;
        }
        self.embedded_replace_index(knowledge_base_id, provider, model, points)
    }

    pub(crate) async fn search(
        &self,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorSearchHit>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if self.postgres.is_some() {
            self.ensure_postgres_schema().await?;
            let pool = self.postgres_pool()?;
            return self
                .postgres_search(pool, knowledge_base_id, provider, model, query, limit)
                .await;
        }
        Ok(self.embedded_search(knowledge_base_id, query, limit))
    }

    pub(crate) async fn remove_index(&self, knowledge_base_id: Uuid) -> Result<(), AppError> {
        if self.postgres.is_some() {
            self.ensure_postgres_schema().await?;
            let pool = self.postgres_pool()?;
            sqlx::query(&format!(
                "DELETE FROM {POSTGRES_TABLE} WHERE knowledge_base_id = $1"
            ))
            .bind(knowledge_base_id)
            .execute(pool)
            .await?;
            return Ok(());
        }
        self.embedded_remove_index(knowledge_base_id)
    }

    fn embedded_current_points(
        &self,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        expected: &[VectorIndexEntry],
    ) -> Option<Vec<VectorIndexPoint>> {
        if !self.ensure_loaded(knowledge_base_id) {
            return None;
        }
        let indexes = self
            .indexes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let index = indexes.get(&knowledge_base_id)?;
        if !matches_index(index, knowledge_base_id, provider, model, expected) {
            return None;
        }
        Some(
            index
                .map
                .iter()
                .map(|(point_id, point)| VectorIndexPoint {
                    entry: index.map.values[point_id.into_inner() as usize].clone(),
                    vector: point.0.clone(),
                })
                .collect(),
        )
    }

    fn embedded_replace_index(
        &self,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        points: Vec<VectorIndexPoint>,
    ) -> Result<(), AppError> {
        let map = Builder::default()
            .ef_search(INDEX_EF_SEARCH)
            .ef_construction(INDEX_EF_CONSTRUCTION)
            .build(
                points
                    .iter()
                    .map(|point| EmbeddingPoint(point.vector.clone()))
                    .collect(),
                points.iter().map(|point| point.entry.clone()).collect(),
            );
        let persisted = PersistedVectorIndex {
            format_version: INDEX_FORMAT_VERSION,
            knowledge_base_id,
            provider: provider.to_string(),
            model: model.to_string(),
            dimensions: points.first().map_or(0, |point| point.vector.len()),
            map,
        };
        let path = self.index_path(knowledge_base_id)?;
        let bytes = bincode::serialize(&persisted).map_err(|error| {
            AppError::Internal(format!("failed to serialize vector index: {error}"))
        })?;
        let directory = self.directory_path()?;
        let temporary_path =
            directory.join(format!("{knowledge_base_id}.index.tmp-{}", Uuid::new_v4()));
        fs::write(&temporary_path, bytes).map_err(|error| {
            AppError::Internal(format!(
                "failed to write vector index {}: {error}",
                temporary_path.display()
            ))
        })?;
        replace_index_file(&temporary_path, &path)?;
        let mut indexes = self
            .indexes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        indexes.insert(knowledge_base_id, persisted);
        Ok(())
    }

    fn embedded_search(
        &self,
        knowledge_base_id: Uuid,
        query: &[f32],
        limit: usize,
    ) -> Vec<VectorSearchHit> {
        if !self.ensure_loaded(knowledge_base_id) {
            return Vec::new();
        }
        let indexes = self
            .indexes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(index) = indexes.get(&knowledge_base_id) else {
            return Vec::new();
        };
        if index.dimensions != query.len() {
            return Vec::new();
        }
        let mut search = Search::default();
        index
            .map
            .search(&EmbeddingPoint(query.to_vec()), &mut search)
            .take(limit)
            .map(|item| VectorSearchHit {
                entry: item.value.clone(),
                score: 1.0 - item.distance,
            })
            .collect()
    }

    fn embedded_remove_index(&self, knowledge_base_id: Uuid) -> Result<(), AppError> {
        self.indexes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&knowledge_base_id);
        let path = self.index_path(knowledge_base_id)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::Internal(format!(
                "failed to remove vector index {}: {error}",
                path.display()
            ))),
        }
    }

    async fn ensure_postgres_schema(&self) -> Result<(), AppError> {
        let pool = self.postgres_pool()?;
        self.postgres_ready
            .get_or_try_init(|| async move {
                sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
                    .execute(pool)
                    .await
                    .map_err(|error| {
                        AppError::Internal(format!(
                            "failed to enable PostgreSQL vector extension: {error}"
                        ))
                    })?;
                sqlx::query(&format!(
                    "CREATE TABLE IF NOT EXISTS {POSTGRES_TABLE} (
                        knowledge_base_id UUID NOT NULL,
                        document_id UUID NOT NULL,
                        chunk_index INTEGER NOT NULL,
                        content_hash VARCHAR(64) NOT NULL,
                        provider VARCHAR(32) NOT NULL,
                        model VARCHAR(255) NOT NULL,
                        dimensions INTEGER NOT NULL,
                        text TEXT NOT NULL,
                        embedding vector NOT NULL,
                        created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                        updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                        PRIMARY KEY (knowledge_base_id, document_id, chunk_index)
                    )"
                ))
                .execute(pool)
                .await
                .map_err(|error| {
                    AppError::Internal(format!("failed to create PostgreSQL vector table: {error}"))
                })?;
                sqlx::query(&format!(
                    "CREATE INDEX IF NOT EXISTS guglerag_vector_embeddings_filter_idx
                     ON {POSTGRES_TABLE} (knowledge_base_id, provider, model, dimensions)"
                ))
                .execute(pool)
                .await
                .map_err(|error| {
                    AppError::Internal(format!(
                        "failed to create PostgreSQL vector filter index: {error}"
                    ))
                })?;
                Ok(())
            })
            .await
            .map(|_| ())
    }

    async fn postgres_current_points(
        &self,
        pool: &sqlx::PgPool,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        expected: &[VectorIndexEntry],
    ) -> Result<Option<Vec<VectorIndexPoint>>, AppError> {
        let rows = sqlx::query(&format!(
            "SELECT document_id, knowledge_base_id, chunk_index, content_hash, text,
                    dimensions, embedding::text AS embedding
             FROM {POSTGRES_TABLE}
             WHERE knowledge_base_id = $1 AND provider = $2 AND model = $3"
        ))
        .bind(knowledge_base_id)
        .bind(provider)
        .bind(model)
        .fetch_all(pool)
        .await?;
        let mut points = Vec::with_capacity(rows.len());
        for row in rows {
            let dimensions: i32 = row.try_get("dimensions")?;
            let vector = decode_vector(&row.try_get::<String, _>("embedding")?)?;
            if dimensions < 0 || dimensions as usize != vector.len() {
                return Err(AppError::Internal(
                    "PostgreSQL vector dimensions do not match stored data".to_string(),
                ));
            }
            let chunk_index: i32 = row.try_get("chunk_index")?;
            if chunk_index < 0 {
                return Err(AppError::Internal(
                    "PostgreSQL vector chunk index is negative".to_string(),
                ));
            }
            points.push(VectorIndexPoint {
                entry: VectorIndexEntry {
                    document_id: row.try_get("document_id")?,
                    knowledge_base_id: row.try_get("knowledge_base_id")?,
                    chunk_index: chunk_index as usize,
                    content_hash: row.try_get("content_hash")?,
                    text: row.try_get("text")?,
                },
                vector,
            });
        }
        if !matches_entries(
            &points
                .iter()
                .map(|point| point.entry.clone())
                .collect::<Vec<_>>(),
            expected,
        ) {
            return Ok(None);
        }
        Ok(Some(points))
    }

    async fn postgres_replace_index(
        &self,
        pool: &sqlx::PgPool,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        points: &[VectorIndexPoint],
        dimensions: usize,
    ) -> Result<(), AppError> {
        let mut transaction = pool.begin().await?;
        sqlx::query(&format!(
            "DELETE FROM {POSTGRES_TABLE} WHERE knowledge_base_id = $1"
        ))
        .bind(knowledge_base_id)
        .execute(&mut *transaction)
        .await?;
        for point in points {
            let chunk_index = i32::try_from(point.entry.chunk_index).map_err(|_| {
                AppError::Internal(
                    "vector chunk index exceeds PostgreSQL integer range".to_string(),
                )
            })?;
            let dimensions = i32::try_from(dimensions).map_err(|_| {
                AppError::Internal("vector dimensions exceed PostgreSQL integer range".to_string())
            })?;
            sqlx::query(&format!(
                "INSERT INTO {POSTGRES_TABLE}
                    (knowledge_base_id, document_id, chunk_index, content_hash, provider,
                     model, dimensions, text, embedding)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::vector)"
            ))
            .bind(point.entry.knowledge_base_id)
            .bind(point.entry.document_id)
            .bind(chunk_index)
            .bind(&point.entry.content_hash)
            .bind(provider)
            .bind(model)
            .bind(dimensions)
            .bind(&point.entry.text)
            .bind(encode_vector(&point.vector))
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        ensure_postgres_hnsw_index(pool, dimensions).await;
        Ok(())
    }

    async fn postgres_search(
        &self,
        pool: &sqlx::PgPool,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorSearchHit>, AppError> {
        if query.is_empty() || query.iter().any(|value| !value.is_finite()) {
            return Ok(Vec::new());
        }
        let dimensions = i32::try_from(query.len()).map_err(|_| {
            AppError::Internal(
                "query vector dimensions exceed PostgreSQL integer range".to_string(),
            )
        })?;
        let distance_expression = if query.len() <= POSTGRES_HNSW_MAX_DIMENSIONS {
            format!(
                "embedding::vector({}) <=> $1::vector({})",
                query.len(),
                query.len()
            )
        } else {
            "embedding <=> $1::vector".to_string()
        };
        let rows = sqlx::query(&format!(
            "SELECT document_id, knowledge_base_id, chunk_index, content_hash, text,
                    1.0::real - ({distance_expression}) AS score
             FROM {POSTGRES_TABLE}
             WHERE knowledge_base_id = $2 AND provider = $3 AND model = $4 AND dimensions = $5
             ORDER BY {distance_expression}
             LIMIT $6"
        ))
        .bind(encode_vector(query))
        .bind(knowledge_base_id)
        .bind(provider)
        .bind(model)
        .bind(dimensions)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(VectorSearchHit {
                    entry: VectorIndexEntry {
                        document_id: row.try_get("document_id")?,
                        knowledge_base_id: row.try_get("knowledge_base_id")?,
                        chunk_index: usize::try_from(row.try_get::<i32, _>("chunk_index")?)
                            .map_err(|_| {
                                AppError::Internal(
                                    "PostgreSQL vector chunk index is negative".to_string(),
                                )
                            })?,
                        content_hash: row.try_get("content_hash")?,
                        text: row.try_get("text")?,
                    },
                    score: row.try_get("score")?,
                })
            })
            .collect()
    }

    fn postgres_pool(&self) -> Result<&sqlx::PgPool, AppError> {
        self.postgres.as_ref().ok_or_else(|| {
            AppError::Internal("PostgreSQL vector database is not configured".to_string())
        })
    }

    fn ensure_loaded(&self, knowledge_base_id: Uuid) -> bool {
        {
            let indexes = self
                .indexes
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if indexes.contains_key(&knowledge_base_id) {
                return true;
            }
        }
        let Ok(path) = self.index_path(knowledge_base_id) else {
            return false;
        };
        let Ok(bytes) = fs::read(path) else {
            return false;
        };
        let Ok(index) = bincode::deserialize::<PersistedVectorIndex>(&bytes) else {
            return false;
        };
        if index.format_version != INDEX_FORMAT_VERSION
            || index.knowledge_base_id != knowledge_base_id
        {
            return false;
        }
        let mut indexes = self
            .indexes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        indexes.insert(knowledge_base_id, index);
        true
    }

    fn directory_path(&self) -> Result<&Path, AppError> {
        self.directory
            .as_deref()
            .map(PathBuf::as_path)
            .ok_or_else(|| {
                AppError::Internal("embedded vector index directory is not configured".to_string())
            })
    }

    fn index_path(&self, knowledge_base_id: Uuid) -> Result<PathBuf, AppError> {
        Ok(self
            .directory_path()?
            .join(format!("{knowledge_base_id}.index")))
    }
}

fn validate_points(
    knowledge_base_id: Uuid,
    points: &[VectorIndexPoint],
) -> Result<usize, AppError> {
    let dimensions = points.first().map_or(0, |point| point.vector.len());
    if points.iter().any(|point| {
        point.entry.knowledge_base_id != knowledge_base_id
            || point.vector.len() != dimensions
            || point.vector.is_empty()
            || point.vector.iter().any(|value| !value.is_finite())
    }) {
        return Err(AppError::Internal(
            "vector index contains inconsistent embedding dimensions or values".to_string(),
        ));
    }
    Ok(dimensions)
}

fn matches_index(
    index: &PersistedVectorIndex,
    knowledge_base_id: Uuid,
    provider: &str,
    model: &str,
    expected: &[VectorIndexEntry],
) -> bool {
    if index.format_version != INDEX_FORMAT_VERSION
        || index.knowledge_base_id != knowledge_base_id
        || index.provider != provider
        || index.model != model
        || index.map.values.len() != expected.len()
    {
        return false;
    }
    let actual = index.map.values.iter().collect::<Vec<_>>();
    matches_entries_ref(&actual, expected)
}

fn matches_entries(actual: &[VectorIndexEntry], expected: &[VectorIndexEntry]) -> bool {
    let actual = actual.iter().collect::<Vec<_>>();
    matches_entries_ref(&actual, expected)
}

fn matches_entries_ref(actual: &[&VectorIndexEntry], expected: &[VectorIndexEntry]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    let mut actual = actual.to_vec();
    let mut expected = expected.iter().collect::<Vec<_>>();
    sort_entries(&mut actual);
    sort_entries(&mut expected);
    actual.iter().zip(expected).all(|(actual, expected)| {
        actual.document_id == expected.document_id
            && actual.knowledge_base_id == expected.knowledge_base_id
            && actual.chunk_index == expected.chunk_index
            && actual.content_hash == expected.content_hash
    })
}

fn sort_entries(entries: &mut [&VectorIndexEntry]) {
    entries.sort_unstable_by(|left, right| {
        left.document_id
            .cmp(&right.document_id)
            .then_with(|| left.chunk_index.cmp(&right.chunk_index))
    });
}

fn encode_vector(vector: &[f32]) -> String {
    let values = vector
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("[{values}]")
}

fn decode_vector(value: &str) -> Result<Vec<f32>, AppError> {
    serde_json::from_str(value)
        .map_err(|error| AppError::Internal(format!("failed to decode PostgreSQL vector: {error}")))
}

async fn ensure_postgres_hnsw_index(pool: &sqlx::PgPool, dimensions: usize) {
    if dimensions == 0 || dimensions > POSTGRES_HNSW_MAX_DIMENSIONS {
        return;
    }
    let index_name = format!("guglerag_vector_embeddings_hnsw_{dimensions}");
    let statement = format!(
        "CREATE INDEX IF NOT EXISTS {index_name}
         ON {POSTGRES_TABLE}
         USING hnsw ((embedding::vector({dimensions})) vector_cosine_ops)
         WHERE dimensions = {dimensions}"
    );
    if let Err(error) = sqlx::query(&statement).execute(pool).await {
        warn!(
            dimensions,
            "failed to create PostgreSQL vector HNSW index: {error}"
        );
    }
}

fn replace_index_file(temporary_path: &Path, index_path: &Path) -> Result<(), AppError> {
    let backup_path = index_path.with_extension(format!("index.bak-{}", Uuid::new_v4()));
    let had_existing = index_path.exists();
    if had_existing {
        fs::rename(index_path, &backup_path).map_err(|error| {
            AppError::Internal(format!(
                "failed to prepare existing vector index for replacement: {error}"
            ))
        })?;
    }
    if let Err(error) = fs::rename(temporary_path, index_path) {
        if had_existing {
            let _ = fs::rename(&backup_path, index_path);
        }
        let _ = fs::remove_file(temporary_path);
        return Err(AppError::Internal(format!(
            "failed to activate vector index: {error}"
        )));
    }
    if had_existing {
        let _ = fs::remove_file(backup_path);
    }
    Ok(())
}
