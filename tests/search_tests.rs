use chrono::{Duration, Utc};
use gugle_rag::{domain::Document, search::search_documents};
use uuid::Uuid;

fn document(title: &str, content: &str, tags: &[&str], age_minutes: i64) -> Document {
    let timestamp = Utc::now() - Duration::minutes(age_minutes);
    Document {
        id: Uuid::new_v4(),
        knowledge_base_id: Uuid::new_v4(),
        title: title.to_string(),
        content: content.to_string(),
        parent_id: None,
        is_folder: false,
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        author_id: Uuid::new_v4(),
        created_at: timestamp,
        updated_at: timestamp,
        versions: Vec::new(),
    }
}

#[test]
fn title_and_tags_rank_above_content_matches() {
    let documents = vec![
        document("Rust handbook", "language notes", &[], 3),
        document("Handbook", "language notes", &["rust"], 2),
        document("Handbook", "a rust language note", &[], 1),
    ];

    let results = search_documents(&documents, "rust", 10);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].title, "Rust handbook");
    assert_eq!(results[0].score, 8.0);
    assert_eq!(results[1].score, 5.0);
    assert_eq!(results[2].score, 1.0);
}

#[test]
fn search_honors_requested_and_global_limits() {
    let documents = (0..55)
        .map(|index| document(&format!("Rust {index}"), "", &[], index))
        .collect::<Vec<_>>();

    assert_eq!(search_documents(&documents, "rust", 2).len(), 2);
    assert_eq!(search_documents(&documents, "rust", 100).len(), 50);
}

#[test]
fn empty_query_returns_no_results() {
    let documents = vec![document("Rust", "content", &[], 0)];
    assert!(search_documents(&documents, "   ", 10).is_empty());
}

#[test]
fn folders_are_not_search_results() {
    let mut folder = document("Rust guides", "", &[], 0);
    folder.is_folder = true;
    assert!(search_documents(&[folder], "rust", 10).is_empty());
}
