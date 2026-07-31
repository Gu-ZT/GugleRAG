use crate::{
    config::Config,
    db::{Database, DocumentEmbeddingChunk, NewDocumentEmbeddingChunk},
    domain::{Document, SearchResult},
    embedding::{EmbeddingService, cosine_similarity},
    error::AppError,
    reranker::RerankerService,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    slice,
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_SEARCH_RESULTS: usize = 50;
const EMBEDDING_BATCH_SIZE: usize = 32;
const DEFAULT_EMBEDDING_CHUNK_SIZE_CHARS: usize = 4_000;
const BGE_LARGE_EMBEDDING_CHUNK_SIZE_CHARS: usize = 384;
const BGE_M3_EMBEDDING_CHUNK_SIZE_CHARS: usize = 6_144;
const QWEN_EMBEDDING_CHUNK_SIZE_CHARS: usize = 8_192;
const EMBEDDING_CHUNK_OVERLAP_CHARS: usize = 400;
const EMBEDDING_HEADER_MAX_CHARS: usize = 800;
const RERANK_CANDIDATE_MULTIPLIER: usize = 5;

#[derive(Clone)]
pub(crate) struct SearchEngine {
    database: Database,
    embedder: EmbeddingService,
    reranker: Option<RerankerService>,
    embedding_lock: Arc<Mutex<()>>,
}

#[derive(Clone)]
struct RankedDocument<'a> {
    document: &'a Document,
    text: String,
    score: f32,
}

#[derive(Clone)]
struct EmbeddingChunkInput {
    chunk_index: usize,
    text: String,
    content_hash: String,
}

#[derive(Clone)]
struct IndexedChunk {
    chunk_index: usize,
    text: String,
    vector: Vec<f32>,
}

struct PendingDocument {
    document_id: Uuid,
    knowledge_base_id: Uuid,
    chunks: Vec<EmbeddingChunkInput>,
}

struct PendingChunk {
    document_id: Uuid,
    chunk_index: usize,
    text: String,
}

impl SearchEngine {
    pub(crate) fn from_config(config: &Config, database: Database) -> Result<Self, AppError> {
        Ok(Self {
            database,
            embedder: EmbeddingService::from_config(config)?,
            reranker: RerankerService::from_config(config)?,
            embedding_lock: Arc::new(Mutex::new(())),
        })
    }

    pub(crate) async fn reindex_all(&self) -> Result<usize, AppError> {
        let documents = self.database.all_documents_for_search().await?;
        let (_, indexed) = self.ensure_embeddings(&documents).await?;
        Ok(indexed)
    }

    pub(crate) async fn search_documents(
        &self,
        documents: &[Document],
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, AppError> {
        let terms = query_terms(query);
        let limit = limit.min(MAX_SEARCH_RESULTS);
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let searchable_documents = documents
            .iter()
            .filter(|document| !document.is_folder)
            .collect::<Vec<_>>();
        if searchable_documents.is_empty() {
            return Ok(Vec::new());
        }

        let (vectors, _) = self.ensure_embeddings(documents).await?;
        let query_text = query
            .trim()
            .chars()
            .take(embedding_chunk_size_chars(self.embedder.model()))
            .collect::<String>();
        if query_text.is_empty() {
            return Ok(Vec::new());
        }
        let query_vector = self
            .embedder
            .embed(slice::from_ref(&query_text))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                AppError::Internal("embedding provider returned no query vector".to_string())
            })?;
        let use_lexical_score = self.embedder.is_stub();
        let mut ranked = searchable_documents
            .into_iter()
            .filter_map(|document| {
                let chunks = vectors.get(&document.id)?;
                let best_chunk = chunks.iter().max_by(|left, right| {
                    cosine_similarity(&left.vector, &query_vector)
                        .total_cmp(&cosine_similarity(&right.vector, &query_vector))
                        .then_with(|| right.chunk_index.cmp(&left.chunk_index))
                })?;
                let semantic_score = cosine_similarity(&best_chunk.vector, &query_vector);
                let score = if use_lexical_score {
                    lexical_score(document, &terms) as f32
                } else {
                    semantic_score
                };
                if use_lexical_score && score == 0.0 {
                    return None;
                }
                Some(RankedDocument {
                    document,
                    text: best_chunk.text.clone(),
                    score,
                })
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| right.document.updated_at.cmp(&left.document.updated_at))
        });

        if let Some(reranker) = &self.reranker {
            let candidate_count = ranked
                .len()
                .min(limit.saturating_mul(RERANK_CANDIDATE_MULTIPLIER).max(limit));
            let candidates = ranked
                .iter()
                .take(candidate_count)
                .cloned()
                .collect::<Vec<_>>();
            let candidate_texts = candidates
                .iter()
                .map(|candidate| candidate.text.clone())
                .collect::<Vec<_>>();
            let mut reranked_scores = reranker
                .rerank(&query_text, &candidate_texts, limit)
                .await?;
            reranked_scores.sort_by(|left, right| right.score.total_cmp(&left.score));
            let mut selected = HashSet::new();
            let mut reranked = Vec::with_capacity(candidates.len());
            for reranked_score in reranked_scores {
                if selected.insert(reranked_score.index)
                    && let Some(mut candidate) = candidates.get(reranked_score.index).cloned()
                {
                    candidate.score = reranked_score.score;
                    reranked.push(candidate);
                }
            }
            for (index, candidate) in candidates.into_iter().enumerate() {
                if selected.insert(index) {
                    reranked.push(candidate);
                }
            }
            ranked = reranked;
        }

        Ok(ranked
            .into_iter()
            .take(limit)
            .map(|result| SearchResult {
                id: result.document.id,
                title: result.document.title.clone(),
                excerpt: excerpt(&result.document.content, &terms),
                score: result.score,
                updated_at: result.document.updated_at,
            })
            .collect())
    }

    async fn ensure_embeddings(
        &self,
        documents: &[Document],
    ) -> Result<(HashMap<Uuid, Vec<IndexedChunk>>, usize), AppError> {
        let _lock = self.embedding_lock.lock().await;
        let mut stored_by_document = HashMap::<Uuid, Vec<DocumentEmbeddingChunk>>::new();
        for embedding in self.database.list_document_embedding_chunks().await? {
            stored_by_document
                .entry(embedding.document_id)
                .or_default()
                .push(embedding);
        }
        let mut vectors = HashMap::new();
        let mut pending_documents = Vec::new();
        let mut pending_chunks = Vec::new();
        for document in documents.iter().filter(|document| !document.is_folder) {
            let chunks = document_embedding_chunks(document, self.embedder.model());
            let mut stored_chunks = stored_by_document.remove(&document.id).unwrap_or_default();
            stored_chunks.sort_by_key(|chunk| chunk.chunk_index);
            let reusable = stored_chunks.len() == chunks.len()
                && chunks.iter().zip(&stored_chunks).all(|(chunk, stored)| {
                    stored.chunk_index == chunk.chunk_index
                        && stored.knowledge_base_id == document.knowledge_base_id
                        && stored.content_hash == chunk.content_hash
                        && stored.provider == self.embedder.provider_name()
                        && stored.model == self.embedder.model()
                });
            if reusable {
                let indexed_chunks = chunks
                    .iter()
                    .zip(stored_chunks)
                    .map(|(chunk, stored)| IndexedChunk {
                        chunk_index: chunk.chunk_index,
                        text: chunk.text.clone(),
                        vector: stored.vector,
                    })
                    .collect();
                vectors.insert(document.id, indexed_chunks);
            } else {
                pending_chunks.extend(chunks.iter().map(|chunk| PendingChunk {
                    document_id: document.id,
                    chunk_index: chunk.chunk_index,
                    text: chunk.text.clone(),
                }));
                pending_documents.push(PendingDocument {
                    document_id: document.id,
                    knowledge_base_id: document.knowledge_base_id,
                    chunks,
                });
            }
        }

        let mut generated_vectors = HashMap::<(Uuid, usize), Vec<f32>>::new();
        for batch in pending_chunks.chunks(EMBEDDING_BATCH_SIZE) {
            let inputs = batch
                .iter()
                .map(|chunk| chunk.text.clone())
                .collect::<Vec<_>>();
            let embeddings = self.embedder.embed(&inputs).await?;
            if embeddings.len() != batch.len() {
                return Err(AppError::Internal(
                    "embedding provider returned an unexpected batch size".to_string(),
                ));
            }
            for (chunk, vector) in batch.iter().zip(embeddings) {
                generated_vectors.insert((chunk.document_id, chunk.chunk_index), vector);
            }
        }

        let mut indexed = 0;
        for pending in pending_documents {
            let document_id = pending.document_id;
            let records = pending
                .chunks
                .iter()
                .map(|chunk| {
                    Ok(NewDocumentEmbeddingChunk {
                        chunk_index: chunk.chunk_index,
                        content_hash: chunk.content_hash.clone(),
                        vector: generated_vectors
                            .remove(&(document_id, chunk.chunk_index))
                            .ok_or_else(|| {
                                AppError::Internal(
                                    "missing generated embedding for document chunk".to_string(),
                                )
                            })?,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            self.database
                .replace_document_embedding_chunks(
                    document_id,
                    pending.knowledge_base_id,
                    self.embedder.provider_name(),
                    self.embedder.model(),
                    &records,
                )
                .await?;
            let indexed_chunks = pending
                .chunks
                .into_iter()
                .zip(records)
                .map(|(chunk, record)| IndexedChunk {
                    chunk_index: chunk.chunk_index,
                    text: chunk.text,
                    vector: record.vector,
                })
                .collect();
            vectors.insert(document_id, indexed_chunks);
            indexed += 1;
        }
        Ok((vectors, indexed))
    }
}

pub fn search_documents(documents: &[Document], query: &str, limit: usize) -> Vec<SearchResult> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return Vec::new();
    }

    let mut results = documents
        .iter()
        .filter(|doc| !doc.is_folder)
        .filter_map(|doc| {
            let score = lexical_score(doc, &terms);
            (score > 0).then(|| SearchResult {
                id: doc.id,
                title: doc.title.clone(),
                excerpt: excerpt(&doc.content, &terms),
                score: score as f32,
                updated_at: doc.updated_at,
            })
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.updated_at.cmp(&a.updated_at))
    });
    results.truncate(limit.min(MAX_SEARCH_RESULTS));
    results
}

fn lexical_score(document: &Document, terms: &[String]) -> usize {
    let title = document.title.to_lowercase();
    let content = document.content.to_lowercase();
    let tags = document.tags.join(" ").to_lowercase();
    terms.iter().fold(0usize, |score, term| {
        score
            + title.matches(term).count() * 8
            + tags.matches(term).count() * 5
            + content.matches(term).count()
    })
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn document_embedding_chunks(document: &Document, model: &str) -> Vec<EmbeddingChunkInput> {
    let chunk_size = embedding_chunk_size_chars(model);
    let header = format!(
        "Title: {}\nTags: {}\n\n",
        document.title,
        document.tags.join(", "),
    )
    .chars()
    .take(EMBEDDING_HEADER_MAX_CHARS.min(chunk_size / 4))
    .collect::<String>();
    let content_chars = document.content.chars().collect::<Vec<_>>();
    let content_chunk_size = chunk_size.saturating_sub(header.chars().count()).max(1);
    let overlap = EMBEDDING_CHUNK_OVERLAP_CHARS.min(content_chunk_size / 4);
    if content_chars.is_empty() {
        return vec![EmbeddingChunkInput {
            chunk_index: 0,
            content_hash: content_hash(&header),
            text: header,
        }];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < content_chars.len() {
        let end = (start + content_chunk_size).min(content_chars.len());
        let content = content_chars[start..end].iter().collect::<String>();
        let text = format!("{header}{content}");
        chunks.push(EmbeddingChunkInput {
            chunk_index: chunks.len(),
            content_hash: content_hash(&text),
            text,
        });
        if end == content_chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }
    chunks
}

fn embedding_chunk_size_chars(model: &str) -> usize {
    // Character limits leave room below the documented token limits because this service does not bundle a tokenizer.
    match model.trim().to_ascii_lowercase().as_str() {
        "baai/bge-large-zh-v1.5"
        | "baai/bge-large-en-v1.5"
        | "netease-youdao/bce-embedding-base_v1" => BGE_LARGE_EMBEDDING_CHUNK_SIZE_CHARS,
        "baai/bge-m3" | "pro/baai/bge-m3" => BGE_M3_EMBEDDING_CHUNK_SIZE_CHARS,
        "qwen/qwen3-embedding-8b" | "qwen/qwen3-embedding-4b" | "qwen/qwen3-embedding-0.6b" => {
            QWEN_EMBEDDING_CHUNK_SIZE_CHARS
        }
        _ => DEFAULT_EMBEDDING_CHUNK_SIZE_CHARS,
    }
}

fn content_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn excerpt(content: &str, terms: &[String]) -> String {
    let lower = content.to_lowercase();
    let start = terms
        .iter()
        .filter_map(|term| lower.find(term))
        .min()
        .unwrap_or(0);
    let char_start = content
        .char_indices()
        .take_while(|(index, _)| *index < start)
        .count()
        .saturating_sub(40);
    content
        .chars()
        .skip(char_start)
        .take(160)
        .collect::<String>()
        .replace('\n', " ")
}
