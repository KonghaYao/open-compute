//! Cloudflare-compatible AI Search result projection.

use super::*;

pub(super) fn project_search_result(
    store: &AiSearchStore,
    chunks: &HashMap<&str, &AiSearchChunkRecord>,
    candidates: &[open_compute_search::ai_search::ScoredCandidate],
    expanded: &HashMap<String, String>,
    retrieval_type: &str,
    fusion: FusionMethod,
    metadata_only: bool,
) -> Result<Vec<Value>, PlatformError> {
    candidates
        .iter()
        .map(|candidate| {
            let chunk = chunks
                .get(candidate.chunk_id.as_str())
                .ok_or_else(corrupt)?;
            let mut scoring_details = Map::new();
            for (name, value) in [
                ("vector_rank", candidate.vector_rank.map(Value::from)),
                ("vector_score", candidate.vector_score.map(Value::from)),
                ("keyword_rank", candidate.keyword_rank.map(Value::from)),
                ("keyword_score", candidate.keyword_score.map(Value::from)),
                (
                    "reranking_score",
                    candidate.reranking_score.map(Value::from),
                ),
            ] {
                if let Some(value) = value {
                    scoring_details.insert(name.to_owned(), value);
                }
            }
            if retrieval_type == "hybrid" {
                scoring_details.insert(
                    "fusion_method".to_owned(),
                    Value::String(
                        match fusion {
                            FusionMethod::Maximum => "max",
                            FusionMethod::ReciprocalRank => "rrf",
                        }
                        .to_owned(),
                    ),
                );
            }
            Ok(json!({
                "id": chunk.id,
                "type": retrieval_type,
                "score": candidate.public_score,
                "text": if metadata_only {
                    ""
                } else {
                    expanded.get(&candidate.chunk_id).map_or(chunk.text.as_str(), String::as_str)
                },
                "item": search_item_value(store, chunk)?,
                "scoring_details": scoring_details,
            }))
        })
        .collect()
}

fn search_item_value(
    store: &AiSearchStore,
    chunk: &AiSearchChunkRecord,
) -> Result<Value, PlatformError> {
    let metadata: Value = serde_json::from_slice(&chunk.metadata_json).map_err(|_| corrupt())?;
    let mut item = json!({
        "timestamp": chunk.item_created_at_ms,
        "key": chunk.item_key,
        "metadata": metadata,
    });
    if let AiSearchSourceReference::Manual(source) =
        store.get_item(&chunk.item_id)?.ok_or_else(corrupt)?.source
    {
        item.as_object_mut().ok_or_else(corrupt)?.insert(
            "open_compute_source".to_owned(),
            json!({
                "provider_id": source.provider_id,
                "source": source.source,
                "key": chunk.item_key,
                "revision": source.revision,
            }),
        );
    }
    Ok(item)
}
