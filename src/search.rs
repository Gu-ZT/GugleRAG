use crate::domain::{Document, SearchResult};

pub fn search_documents(documents: &[Document], query: &str, limit: usize) -> Vec<SearchResult> {
    let terms = query
        .to_lowercase()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Vec::new();
    }

    let mut results = documents
        .iter()
        .filter(|doc| !doc.is_folder)
        .filter_map(|doc| {
            let title = doc.title.to_lowercase();
            let content = doc.content.to_lowercase();
            let tags = doc.tags.join(" ").to_lowercase();
            let score = terms.iter().fold(0usize, |score, term| {
                score
                    + title.matches(term).count() * 8
                    + tags.matches(term).count() * 5
                    + content.matches(term).count()
            });
            (score > 0).then(|| SearchResult {
                id: doc.id,
                title: doc.title.clone(),
                excerpt: excerpt(&doc.content, &terms),
                score,
                updated_at: doc.updated_at,
            })
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| b.score.cmp(&a.score).then(b.updated_at.cmp(&a.updated_at)));
    results.truncate(limit.min(50));
    results
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
