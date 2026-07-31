use crate::{
    config::Config,
    db::Database,
    domain::{Document, SearchResult},
    embedding::{EmbeddingService, cosine_similarity},
    error::AppError,
    reranker::RerankerService,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_SEARCH_RESULTS: usize = 50;
const EMBEDDING_BATCH_SIZE: usize = 32;
const MAX_EMBEDDING_TEXT_CHARS: usize = 16_000;
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
        let query_vector = self
            .embedder
            .embed(&[query.to_string()])
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
                let vector = vectors.get(&document.id)?;
                let semantic_score = cosine_similarity(vector, &query_vector);
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
                    text: document_embedding_text(document),
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
            let mut reranked_scores = reranker.rerank(query, &candidate_texts, limit).await?;
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
    ) -> Result<(HashMap<Uuid, Vec<f32>>, usize), AppError> {
        let _lock = self.embedding_lock.lock().await;
        let stored = self
            .database
            .list_document_embeddings()
            .await?
            .into_iter()
            .map(|embedding| (embedding.document_id, embedding))
            .collect::<HashMap<_, _>>();
        let mut vectors = HashMap::new();
        let mut missing = Vec::new();
        for document in documents.iter().filter(|document| !document.is_folder) {
            let text = document_embedding_text(document);
            let content_hash = content_hash(&text);
            if let Some(embedding) = stored.get(&document.id)
                && embedding.knowledge_base_id == document.knowledge_base_id
                && embedding.content_hash == content_hash
                && embedding.provider == self.embedder.provider_name()
                && embedding.model == self.embedder.model()
            {
                vectors.insert(document.id, embedding.vector.clone());
            } else {
                missing.push((document, text, content_hash));
            }
        }

        let mut indexed = 0;
        for batch in missing.chunks(EMBEDDING_BATCH_SIZE) {
            let inputs = batch
                .iter()
                .map(|(_, text, _)| text.clone())
                .collect::<Vec<_>>();
            let embeddings = self.embedder.embed(&inputs).await?;
            if embeddings.len() != batch.len() {
                return Err(AppError::Internal(
                    "embedding provider returned an unexpected batch size".to_string(),
                ));
            }
            for ((document, _, content_hash), vector) in batch.iter().zip(embeddings) {
                self.database
                    .replace_document_embedding(
                        document.id,
                        document.knowledge_base_id,
                        content_hash,
                        self.embedder.provider_name(),
                        self.embedder.model(),
                        &vector,
                    )
                    .await?;
                vectors.insert(document.id, vector);
                indexed += 1;
            }
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

fn document_embedding_text(document: &Document) -> String {
    let text = format!(
        "Title: {}\nTags: {}\n\n{}",
        document.title,
        document.tags.join(", "),
        document.content
    );
    text.chars().take(MAX_EMBEDDING_TEXT_CHARS).collect()
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
