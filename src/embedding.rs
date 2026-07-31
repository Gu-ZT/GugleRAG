use crate::{config::Config, error::AppError};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STUB_DIMENSIONS: usize = 384;

#[derive(Clone)]
pub(crate) struct EmbeddingService {
    provider: EmbeddingProvider,
    provider_name: String,
    model: String,
}

#[derive(Clone)]
enum EmbeddingProvider {
    Stub,
    Http {
        client: Client,
        url: String,
        api_key: Option<String>,
    },
}

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
    encoding_format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncate: Option<bool>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}

impl EmbeddingService {
    pub(crate) fn from_config(config: &Config) -> Result<Self, AppError> {
        let provider_name = config.embedding_provider.trim().to_lowercase();
        let model = config.embedding_model.trim().to_string();
        if provider_name != "stub" && model.is_empty() {
            return Err(AppError::Internal(
                "EMBEDDING_MODEL is required when embeddings are enabled".to_string(),
            ));
        }
        let provider = match provider_name.as_str() {
            "stub" => EmbeddingProvider::Stub,
            "siliconflow" => {
                if config.siliconflow_api_key.trim().is_empty() {
                    return Err(AppError::Internal(
                        "SILICONFLOW_API_KEY is required for siliconflow embeddings".to_string(),
                    ));
                }
                let url = if config.embedding_url.trim().is_empty() {
                    endpoint_url(&config.siliconflow_url, "/v1/embeddings")
                } else {
                    config.embedding_url.trim().to_string()
                };
                if url.is_empty() {
                    return Err(AppError::Internal(
                        "EMBEDDING_URL is required for siliconflow embeddings".to_string(),
                    ));
                }
                EmbeddingProvider::Http {
                    client: Client::new(),
                    url,
                    api_key: Some(config.siliconflow_api_key.clone()),
                }
            }
            "local" => {
                let url = config.embedding_url.trim().to_string();
                if url.is_empty() {
                    return Err(AppError::Internal(
                        "EMBEDDING_URL is required for local embeddings".to_string(),
                    ));
                }
                EmbeddingProvider::Http {
                    client: Client::new(),
                    url,
                    api_key: None,
                }
            }
            _ => {
                return Err(AppError::Internal(format!(
                    "unsupported embedding provider: {provider_name}"
                )));
            }
        };
        Ok(Self {
            provider,
            provider_name,
            model,
        })
    }

    pub(crate) fn provider_name(&self) -> &str {
        &self.provider_name
    }

    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    pub(crate) fn is_stub(&self) -> bool {
        matches!(self.provider, EmbeddingProvider::Stub)
    }

    pub(crate) async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let vectors = match &self.provider {
            EmbeddingProvider::Stub => {
                Ok(inputs.iter().map(|input| stub_embedding(input)).collect())
            }
            EmbeddingProvider::Http {
                client,
                url,
                api_key,
            } => {
                self.embed_http(client, url, api_key.as_deref(), inputs)
                    .await
            }
        }?;
        validate_dimensions(&vectors)?;
        Ok(vectors)
    }

    async fn embed_http(
        &self,
        client: &Client,
        url: &str,
        api_key: Option<&str>,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>, AppError> {
        if api_key.is_some_and(|key| key.trim().is_empty()) {
            return Err(AppError::Internal(
                "SILICONFLOW_API_KEY is required for embeddings".to_string(),
            ));
        }
        let request = EmbeddingRequest {
            model: &self.model,
            input: inputs,
            encoding_format: "float",
            // SiliconFlow rejects inputs above the model limit unless truncation is enabled.
            truncate: api_key.is_some().then_some(true),
        };
        let mut builder = client.post(url).json(&request);
        if let Some(api_key) = api_key {
            builder = builder.bearer_auth(api_key);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| AppError::Internal(format!("embedding request failed: {error}")))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            AppError::Internal(format!("failed to read embedding response: {error}"))
        })?;
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "embedding provider returned {status}: {}",
                truncate_body(&body)
            )));
        }
        let parsed: EmbeddingResponse = serde_json::from_str(&body)
            .map_err(|error| AppError::Internal(format!("invalid embedding response: {error}")))?;
        if parsed.data.len() != inputs.len() {
            return Err(AppError::Internal(format!(
                "embedding provider returned {} vectors for {} inputs",
                parsed.data.len(),
                inputs.len()
            )));
        }
        parsed
            .data
            .into_iter()
            .map(|item| {
                if item.embedding.is_empty() {
                    Err(AppError::Internal(
                        "embedding provider returned an empty vector".to_string(),
                    ))
                } else {
                    Ok(item.embedding)
                }
            })
            .collect()
    }
}

fn stub_embedding(input: &str) -> Vec<f32> {
    let mut vector = vec![0.0; STUB_DIMENSIONS];
    for token in input.split_whitespace() {
        let digest = Sha256::digest(token.to_lowercase().as_bytes());
        for chunk in digest.chunks_exact(2).take(8) {
            let bucket = usize::from(u16::from_be_bytes([chunk[0], chunk[1]])) % STUB_DIMENSIONS;
            let sign = if chunk[0] & 1 == 0 { 1.0 } else { -1.0 };
            vector[bucket] += sign;
        }
    }
    normalize(&mut vector);
    vector
}

pub(crate) fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let dot = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    }
}

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

fn validate_dimensions(vectors: &[Vec<f32>]) -> Result<(), AppError> {
    let Some(first) = vectors.first() else {
        return Ok(());
    };
    if vectors.iter().any(|vector| vector.len() != first.len()) {
        return Err(AppError::Internal(
            "embedding provider returned vectors with inconsistent dimensions".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn endpoint_url(base: &str, suffix: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with(suffix) {
        base.to_string()
    } else if base.ends_with("/v1") && suffix.starts_with("/v1/") {
        format!("{base}{}", &suffix[3..])
    } else {
        format!("{base}{suffix}")
    }
}

fn truncate_body(body: &str) -> String {
    body.chars().take(500).collect()
}
