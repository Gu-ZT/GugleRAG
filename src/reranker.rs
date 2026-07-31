use crate::{config::Config, embedding::endpoint_url, error::AppError};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub(crate) struct RerankerService {
    client: Client,
    url: String,
    api_key: Option<String>,
    model: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RerankScore {
    pub(crate) index: usize,
    pub(crate) score: f32,
}

#[derive(Serialize)]
struct RerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: &'a [String],
    top_n: usize,
    return_documents: bool,
}

#[derive(Deserialize)]
struct RerankResponse {
    #[serde(default)]
    results: Vec<RerankItem>,
    #[serde(default)]
    data: Vec<RerankItem>,
}

#[derive(Deserialize)]
struct RerankItem {
    index: usize,
    #[serde(default)]
    score: Option<f32>,
    #[serde(default)]
    relevance_score: Option<f32>,
}

impl RerankerService {
    pub(crate) fn from_config(config: &Config) -> Result<Option<Self>, AppError> {
        if !config.reranker_enabled {
            return Ok(None);
        }
        let model = config.reranker_model.trim().to_string();
        if model.is_empty() {
            return Err(AppError::Internal(
                "RERANKER_MODEL is required when reranker is enabled".to_string(),
            ));
        }
        let (url, api_key) = match config.reranker_provider.trim().to_lowercase().as_str() {
            "siliconflow" => {
                if config.siliconflow_api_key.trim().is_empty() {
                    return Err(AppError::Internal(
                        "SILICONFLOW_API_KEY is required for siliconflow reranking".to_string(),
                    ));
                }
                (
                    endpoint_url(&config.siliconflow_url, "/v1/rerank"),
                    Some(config.siliconflow_api_key.clone()),
                )
            }
            "local" | "custom_http" => {
                let url = config.reranker_url.trim().to_string();
                if url.is_empty() {
                    let provider = config.reranker_provider.trim();
                    let message = if provider == "custom_http" {
                        "RERANKER_URL is required for custom_http reranker"
                    } else {
                        "RERANKER_URL is required for local reranker"
                    };
                    return Err(AppError::Internal(message.to_string()));
                }
                (url, None)
            }
            provider => {
                return Err(AppError::Internal(format!(
                    "unsupported reranker provider: {provider}"
                )));
            }
        };
        Ok(Some(Self {
            client: Client::new(),
            url,
            api_key,
            model,
        }))
    }

    pub(crate) async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_n: usize,
    ) -> Result<Vec<RerankScore>, AppError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        if self
            .api_key
            .as_deref()
            .is_some_and(|key| key.trim().is_empty())
        {
            return Err(AppError::Internal(
                "SILICONFLOW_API_KEY is required for reranking".to_string(),
            ));
        }
        let request = RerankRequest {
            model: &self.model,
            query,
            documents,
            top_n,
            return_documents: false,
        };
        let mut builder = self.client.post(&self.url).json(&request);
        if let Some(api_key) = self.api_key.as_deref() {
            builder = builder.bearer_auth(api_key);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| AppError::Internal(format!("reranker request failed: {error}")))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            AppError::Internal(format!("failed to read reranker response: {error}"))
        })?;
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "reranker provider returned {status}: {}",
                body.chars().take(500).collect::<String>()
            )));
        }
        let response: RerankResponse = serde_json::from_str(&body)
            .map_err(|error| AppError::Internal(format!("invalid reranker response: {error}")))?;
        let items = if response.results.is_empty() {
            response.data
        } else {
            response.results
        };
        items
            .into_iter()
            .map(|item| {
                let score = item.relevance_score.or(item.score).ok_or_else(|| {
                    AppError::Internal("reranker result has no score".to_string())
                })?;
                if item.index >= documents.len() {
                    return Err(AppError::Internal(format!(
                        "reranker returned invalid document index {}",
                        item.index
                    )));
                }
                Ok(RerankScore {
                    index: item.index,
                    score,
                })
            })
            .collect()
    }
}
