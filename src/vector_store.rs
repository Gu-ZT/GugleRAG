use crate::{embedding::cosine_similarity, error::AppError};
use instant_distance::{Builder, HnswMap, Point, Search};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use uuid::Uuid;

const INDEX_FORMAT_VERSION: u32 = 1;
const INDEX_EF_SEARCH: usize = 256;
const INDEX_EF_CONSTRUCTION: usize = 256;

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
    directory: Arc<PathBuf>,
    indexes: Arc<RwLock<HashMap<Uuid, PersistedVectorIndex>>>,
}

impl VectorStore {
    pub(crate) fn new(directory: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(&directory).map_err(|error| {
            AppError::Internal(format!(
                "failed to create vector index directory {}: {error}",
                directory.display()
            ))
        })?;
        Ok(Self {
            directory: Arc::new(directory),
            indexes: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub(crate) fn current_points(
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

    pub(crate) fn replace_index(
        &self,
        knowledge_base_id: Uuid,
        provider: &str,
        model: &str,
        points: Vec<VectorIndexPoint>,
    ) -> Result<(), AppError> {
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
            dimensions,
            map,
        };
        let path = self.index_path(knowledge_base_id);
        let bytes = bincode::serialize(&persisted).map_err(|error| {
            AppError::Internal(format!("failed to serialize vector index: {error}"))
        })?;
        let temporary_path = self
            .directory
            .join(format!("{knowledge_base_id}.index.tmp-{}", Uuid::new_v4()));
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

    pub(crate) fn search(
        &self,
        knowledge_base_id: Uuid,
        query: &[f32],
        limit: usize,
    ) -> Vec<VectorSearchHit> {
        if limit == 0 || !self.ensure_loaded(knowledge_base_id) {
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

    pub(crate) fn remove_index(&self, knowledge_base_id: Uuid) -> Result<(), AppError> {
        self.indexes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&knowledge_base_id);
        let path = self.index_path(knowledge_base_id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::Internal(format!(
                "failed to remove vector index {}: {error}",
                path.display()
            ))),
        }
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
        let path = self.index_path(knowledge_base_id);
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

    fn index_path(&self, knowledge_base_id: Uuid) -> PathBuf {
        self.directory.join(format!("{knowledge_base_id}.index"))
    }
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
    let mut actual = index.map.values.iter().collect::<Vec<_>>();
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
