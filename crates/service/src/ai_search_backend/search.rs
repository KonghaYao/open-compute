//! AI Search retrieval, chat generation, and SSE response composition.

use super::*;

impl AiSearchBindingService {
    pub(super) async fn instance_search(
        &self,
        authority: &Authority,
        call: JsonCall,
    ) -> Result<Value, PlatformError> {
        let payload: SearchPayload =
            serde_json::from_value(call.payload).map_err(|_| protocol())?;
        let instance = self.resolve_instance(authority, call.instance.as_deref())?;
        self.search_record(&instance.record, &payload, None).await
    }

    pub(super) async fn namespace_search(
        &self,
        authority: &Authority,
        call: JsonCall,
    ) -> Result<Value, PlatformError> {
        require_namespace(authority)?;
        if call.instance.is_some() {
            return Err(protocol());
        }
        let payload: SearchPayload =
            serde_json::from_value(call.payload).map_err(|_| protocol())?;
        let instance_ids = payload
            .ai_search_options
            .instance_ids
            .as_ref()
            .filter(|ids| !ids.is_empty() && ids.len() <= 10)
            .cloned()
            .ok_or_else(protocol)?;
        if instance_ids.iter().collect::<BTreeSet<_>>().len() != instance_ids.len() {
            return Err(protocol());
        }
        let return_on_failure = payload.ai_search_options.return_on_failure();
        let search_query = payload.query_text()?;
        let payload = Arc::new(payload);
        let instance_id = authority.instance_id;
        let namespace_id = authority.resource.id;
        let shared_embeddings = new_query_embedding_cache();
        let results =
            futures::stream::iter(instance_ids)
                .map(|key| {
                    let payload = payload.clone();
                    let shared_embeddings = shared_embeddings.clone();
                    async move {
                        let result = match AiSearchCatalog::new(self.storage.db())
                            .get_instance_by_key(instance_id, namespace_id, &key)
                        {
                            Ok(record) => match self.pins.try_pin(record.resource.id) {
                                Ok(_pin) => {
                                    self.search_record(
                                        &record,
                                        payload.as_ref(),
                                        Some(&shared_embeddings),
                                    )
                                    .await
                                }
                                Err(error) => Err(error),
                            },
                            Err(error) => Err(error),
                        };
                        (key, result)
                    }
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
        let mut chunks = Vec::new();
        let mut errors = Vec::new();
        for (key, result) in results {
            match result {
                Ok(value) => {
                    let values = value
                        .get("chunks")
                        .and_then(Value::as_array)
                        .ok_or_else(corrupt)?;
                    for value in values {
                        let mut value = value.as_object().cloned().ok_or_else(corrupt)?;
                        value.insert("instance_id".to_owned(), Value::String(key.clone()));
                        chunks.push(Value::Object(value));
                    }
                }
                Err(error) if return_on_failure => errors.push(json!({
                    "instance_id": key,
                    "message": error.code().as_str(),
                })),
                Err(error) => return Err(error),
            }
        }
        errors.sort_by(|left, right| {
            left["instance_id"]
                .as_str()
                .cmp(&right["instance_id"].as_str())
        });
        chunks.sort_by(|left, right| {
            let left = left.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            let right = right.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            right.total_cmp(&left)
        });
        chunks.truncate(50);
        Ok(json!({
            "search_query": search_query,
            "query_kind": "text",
            "chunks": chunks,
            "errors": errors,
        }))
    }

    pub(super) async fn search_record(
        &self,
        record: &AiSearchInstanceRecord,
        payload: &SearchPayload,
        shared_embeddings: Option<&SharedQueryEmbeddings>,
    ) -> Result<Value, PlatformError> {
        let generation_lock = self.generation_lock(record.resource.id)?;
        let _generation = tokio::time::timeout(
            Duration::from_millis(self.ai.query_timeout_ms),
            generation_lock.read_owned(),
        )
        .await
        .map_err(|_| query_timeout())?;
        let (search_store, inspection) = self.open_store(record)?;
        let active_index_generation = inspection.active_index_generation;
        let active_epoch = inspection.active_epoch;
        let config: ResolvedAiSearchConfig =
            serde_json::from_slice(&inspection.public_config_json).map_err(|_| corrupt())?;
        let query = self.rewritten_query(payload, &config).await?;
        let RetrievalPlan {
            retrieval_type,
            filter,
            maximum,
            threshold,
            boosts,
            keyword_query,
        } = retrieval_plan(payload, &config, &query)?;
        let retrieval_type = retrieval_type.as_str();
        let retrieval = payload.ai_search_options.retrieval.as_ref();
        const MAX_BRANCH_CANDIDATES: usize = 256;
        let keyword_task = if let Some((fts_query, trigram)) = keyword_query {
            let query_service = self.clone();
            let query_record = record.clone();
            let keyword_filter = filter.clone();
            Some(tokio::task::spawn_blocking(move || {
                let (query_store, _) = query_service.open_store(&query_record)?;
                let mut chunks = Vec::new();
                query_store.scan_keyword_chunks_at(
                    active_index_generation,
                    &fts_query,
                    trigram,
                    |chunk, score| {
                        if metadata_matches(&chunk, keyword_filter.as_ref()) {
                            chunks.push((chunk, score));
                        }
                        Ok(chunks.len() < MAX_BRANCH_CANDIDATES)
                    },
                )?;
                Ok::<_, PlatformError>(chunks)
            }))
        } else {
            None
        };
        let mut chunks = Vec::new();
        let mut vector = Vec::new();
        if matches!(retrieval_type, "vector" | "hybrid") && inspection.active_chunk_count != 0 {
            let contract: ResolvedEmbeddingModelContract =
                serde_json::from_slice(&inspection.model_contract_json).map_err(|_| corrupt())?;
            let key = (contract.contract_sha256.clone(), query.clone());
            let query_vector = if let Some(shared) = shared_embeddings {
                cached_query_embedding(shared, key, || async {
                    let _permit = self.provider_permit().await?;
                    OpenAiProviderClient::new(&self.ai, &contract)
                        .map_err(provider_error)?
                        .embeddings(std::slice::from_ref(&query))
                        .await
                        .map_err(provider_error)?
                        .embeddings
                        .into_iter()
                        .next()
                        .ok_or_else(corrupt)
                })
                .await?
            } else {
                let _permit = self.provider_permit().await?;
                OpenAiProviderClient::new(&self.ai, &contract)
                    .map_err(provider_error)?
                    .embeddings(std::slice::from_ref(&query))
                    .await
                    .map_err(provider_error)?
                    .embeddings
                    .into_iter()
                    .next()
                    .ok_or_else(corrupt)?
            };
            let vector_service = self.clone();
            let vector_record = record.clone();
            let vector_filter = filter.clone();
            let vector_threshold = threshold as f32;
            let ranked = tokio::task::spawn_blocking(move || {
                let (vector_store, _) = vector_service.open_store(&vector_record)?;
                let mut ranked = Vec::<(RankedCandidate, AiSearchChunkRecord)>::new();
                vector_store.scan_active_chunks_at(active_index_generation, |chunk| {
                    if !metadata_matches(&chunk, vector_filter.as_ref()) {
                        return Ok(());
                    }
                    let embedding = chunk.embedding.as_ref().ok_or_else(corrupt)?;
                    let cosine =
                        cosine_similarity(&query_vector, embedding).map_err(|_| corrupt())?;
                    let score = ((cosine + 1.0) / 2.0).clamp(0.0, 1.0);
                    if score < vector_threshold {
                        return Ok(());
                    }
                    ranked.push((
                        RankedCandidate {
                            chunk_id: chunk.id.clone(),
                            score,
                            reported_score: score,
                        },
                        chunk,
                    ));
                    ranked.sort_by(|left, right| {
                        right
                            .0
                            .score
                            .total_cmp(&left.0.score)
                            .then_with(|| left.0.chunk_id.cmp(&right.0.chunk_id))
                    });
                    ranked.truncate(MAX_BRANCH_CANDIDATES);
                    Ok(())
                })?;
                Ok::<_, PlatformError>(ranked)
            })
            .await
            .map_err(|_| unavailable())??;
            for (candidate, chunk) in ranked {
                vector.push(candidate);
                chunks.push(chunk);
            }
        }
        let mut keyword = if let Some(keyword_task) = keyword_task {
            let mut keyword_chunks = keyword_task.await.map_err(|_| unavailable())??;
            keyword_chunks.retain(|(chunk, _)| metadata_matches(chunk, filter.as_ref()));
            let maximum_score = keyword_chunks
                .iter()
                .map(|(_, score)| *score)
                .fold(0.0_f32, f32::max);
            let ranked = keyword_chunks
                .iter()
                .map(|(chunk, score)| RankedCandidate {
                    chunk_id: chunk.id.clone(),
                    score: if maximum_score == 0.0 {
                        1.0
                    } else {
                        score / maximum_score
                    },
                    reported_score: *score,
                })
                .collect();
            let existing = chunks
                .iter()
                .map(|chunk| chunk.id.clone())
                .collect::<BTreeSet<_>>();
            chunks.extend(
                keyword_chunks
                    .into_iter()
                    .map(|(chunk, _)| chunk)
                    .filter(|chunk| !existing.contains(&chunk.id)),
            );
            ranked
        } else {
            Vec::new()
        };
        sort_ranked(&mut keyword);
        let configured_fusion = match retrieval
            .and_then(|options| options.fusion_method.as_deref())
            .or(match config.fusion_method {
                AiSearchFusionMethod::Max => Some("max"),
                AiSearchFusionMethod::Rrf => Some("rrf"),
            }) {
            Some("max") => FusionMethod::Maximum,
            _ => FusionMethod::ReciprocalRank,
        };
        let fusion = if retrieval_type == "hybrid" {
            configured_fusion
        } else {
            FusionMethod::Maximum
        };
        let mut fused = fuse_candidates(&vector, &keyword, fusion, 50).map_err(|_| protocol())?;
        let rerank = payload
            .ai_search_options
            .reranking
            .as_ref()
            .and_then(|options| options.enabled)
            .unwrap_or(config.reranking);
        let context_expansion = retrieval
            .and_then(|options| options.context_expansion)
            .unwrap_or(0);
        if context_expansion > 3 {
            return Err(limit());
        }
        let by_id = chunks
            .iter()
            .map(|chunk| (chunk.id.as_str(), chunk))
            .collect::<HashMap<_, _>>();
        apply_boosting(&mut fused, &by_id, &boosts, &config.custom_metadata)?;
        if rerank && !fused.is_empty() {
            let alias = payload
                .ai_search_options
                .reranking
                .as_ref()
                .and_then(|options| options.model.as_deref())
                .or(config.reranking_model.as_deref())
                .ok_or_else(unsupported)?;
            let texts = fused
                .iter()
                .map(|candidate| {
                    by_id
                        .get(candidate.chunk_id.as_str())
                        .map(|chunk| chunk.text.clone())
                        .ok_or_else(corrupt)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let _permit = self.provider_permit().await?;
            let results = RerankClient::new(&self.ai, alias)
                .map_err(provider_error)?
                .rerank(&query, &texts)
                .await
                .map_err(provider_error)?;
            fused = results
                .into_iter()
                .map(|result| {
                    let mut candidate = fused[result.index].clone();
                    candidate.reranking_score = Some(result.relevance_score);
                    candidate.public_score = result.relevance_score;
                    candidate
                })
                .collect();
            let threshold = payload
                .ai_search_options
                .reranking
                .as_ref()
                .and_then(|options| options.match_threshold)
                .unwrap_or(0.4);
            fused.retain(|candidate| {
                candidate
                    .reranking_score
                    .is_some_and(|score| f64::from(score) >= threshold)
            });
        }
        fused.truncate(usize::from(maximum));
        let expanded = if context_expansion == 0 || fused.is_empty() {
            HashMap::new()
        } else {
            let targets = fused
                .iter()
                .map(|candidate| {
                    by_id.get(candidate.chunk_id.as_str()).ok_or_else(corrupt)?;
                    Ok(candidate.chunk_id.clone())
                })
                .collect::<Result<Vec<_>, PlatformError>>()?;
            let context_service = self.clone();
            let context_record = record.clone();
            tokio::task::spawn_blocking(move || {
                let (context_store, _) = context_service.open_store(&context_record)?;
                let mut expanded = HashMap::new();
                for chunk_id in targets {
                    let text = context_store
                        .active_chunk_context_at(
                            active_index_generation,
                            &chunk_id,
                            context_expansion,
                        )?
                        .into_iter()
                        .map(|chunk| chunk.text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    expanded.insert(chunk_id, text);
                }
                Ok::<_, PlatformError>(expanded)
            })
            .await
            .map_err(|_| unavailable())??
        };
        let metadata_only = retrieval
            .and_then(|options| options.metadata_only)
            .unwrap_or(false);
        let result = project_search_result(
            &search_store,
            &by_id,
            &fused,
            &expanded,
            retrieval_type,
            fusion,
            metadata_only,
        )?;
        if !search_store.active_fence_matches(active_index_generation, active_epoch)? {
            return Err(unavailable());
        }
        Ok(json!({"search_query": query, "query_kind": "text", "chunks": result}))
    }

    async fn rewritten_query(
        &self,
        payload: &SearchPayload,
        config: &ResolvedAiSearchConfig,
    ) -> Result<String, PlatformError> {
        let query = payload.query_text()?;
        let rewrite = payload
            .ai_search_options
            .query_rewrite
            .as_ref()
            .and_then(|options| options.enabled)
            .unwrap_or(config.rewrite_query);
        if !rewrite {
            return Ok(query);
        }
        let alias = payload
            .ai_search_options
            .query_rewrite
            .as_ref()
            .and_then(|options| options.model.as_deref())
            .or(config.rewrite_model.as_deref())
            .or(config.ai_search_model.as_deref())
            .ok_or_else(unsupported)?;
        let _permit = self.provider_permit().await?;
        OpenAiChatClient::new(&self.ai, alias, AiGenerationCapability::Rewrite)
            .map_err(provider_error)?
            .rewrite_query(&query)
            .await
            .map_err(provider_error)
    }
}

struct RetrievalPlan {
    retrieval_type: String,
    filter: Option<FilterExpr>,
    maximum: u8,
    threshold: f64,
    boosts: Vec<AiSearchBoost>,
    keyword_query: Option<(String, bool)>,
}

fn retrieval_plan(
    payload: &SearchPayload,
    config: &ResolvedAiSearchConfig,
    query: &str,
) -> Result<RetrievalPlan, PlatformError> {
    let retrieval = payload.ai_search_options.retrieval.as_ref();
    let retrieval_type = retrieval
        .and_then(|options| options.retrieval_type.clone())
        .unwrap_or_else(|| {
            if config.index_method.vector && config.index_method.keyword {
                "hybrid"
            } else if config.index_method.vector {
                "vector"
            } else {
                "keyword"
            }
            .to_owned()
        });
    if matches!(retrieval_type.as_str(), "vector" | "hybrid") && !config.index_method.vector
        || matches!(retrieval_type.as_str(), "keyword" | "hybrid") && !config.index_method.keyword
        || !matches!(retrieval_type.as_str(), "vector" | "keyword" | "hybrid")
    {
        return Err(unsupported());
    }
    let filter = if let Some(filter) = retrieval.and_then(|options| options.filters.as_ref()) {
        let indexed = config
            .custom_metadata
            .iter()
            .map(|field| field.field_name.clone())
            .collect::<BTreeSet<_>>();
        Some(compile_filter(filter, &indexed).map_err(|_| protocol())?)
    } else {
        None
    };
    let maximum = retrieval
        .and_then(|options| options.max_num_results)
        .unwrap_or(config.max_num_results);
    let threshold = retrieval
        .and_then(|options| options.match_threshold)
        .unwrap_or(config.score_threshold);
    let boosts = retrieval
        .and_then(|options| options.boost_by.clone())
        .unwrap_or_else(|| config.retrieval_options.boost_by.clone());
    validate_boosts(&boosts, &config.custom_metadata)?;
    let keyword_query = if matches!(retrieval_type.as_str(), "keyword" | "hybrid") {
        let mode = match retrieval.and_then(|options| options.keyword_match_mode.as_deref()) {
            Some("and") => FtsKeywordMatchMode::And,
            Some("or") => FtsKeywordMatchMode::Or,
            Some(_) => return Err(protocol()),
            None => match config.retrieval_options.keyword_match_mode {
                Some(AiSearchKeywordMatchMode::Or) => FtsKeywordMatchMode::Or,
                Some(AiSearchKeywordMatchMode::And) | None => FtsKeywordMatchMode::And,
            },
        };
        let fts_query = build_fts_query(query, mode, 64).map_err(|_| protocol())?;
        let trigram = matches!(
            config.indexing_options.keyword_tokenizer,
            Some(AiSearchKeywordTokenizer::Trigram)
        );
        Some((fts_query, trigram))
    } else {
        None
    };
    Ok(RetrievalPlan {
        retrieval_type,
        filter,
        maximum,
        threshold,
        boosts,
        keyword_query,
    })
}

const MAX_METADATA_BOOST: f32 = 0.3;

fn apply_boosting(
    candidates: &mut [open_compute_search::ai_search::ScoredCandidate],
    chunks: &HashMap<&str, &AiSearchChunkRecord>,
    boosts: &[AiSearchBoost],
    fields: &[crate::ai_search_config::AiSearchMetadataField],
) -> Result<(), PlatformError> {
    if boosts.is_empty() || candidates.is_empty() {
        return Ok(());
    }
    let mut totals = vec![0.0_f32; candidates.len()];
    for boost in boosts {
        let field_type = if boost.field.eq_ignore_ascii_case("timestamp") {
            AiSearchMetadataType::Datetime
        } else {
            fields
                .iter()
                .find(|field| field.field_name.eq_ignore_ascii_case(&boost.field))
                .map(|field| field.data_type)
                .ok_or_else(protocol)?
        };
        let direction = boost.direction.unwrap_or(match field_type {
            AiSearchMetadataType::Number | AiSearchMetadataType::Datetime => {
                AiSearchBoostDirection::Asc
            }
            AiSearchMetadataType::Text | AiSearchMetadataType::Boolean => {
                AiSearchBoostDirection::Exists
            }
        });
        let values = candidates
            .iter()
            .map(|candidate| {
                chunks
                    .get(candidate.chunk_id.as_str())
                    .ok_or_else(corrupt)
                    .and_then(|chunk| boost_value(chunk, &boost.field, field_type))
            })
            .collect::<Result<Vec<_>, _>>()?;
        match direction {
            AiSearchBoostDirection::Exists | AiSearchBoostDirection::NotExists => {
                for (index, value) in values.iter().enumerate() {
                    let present = value.is_some();
                    if present == (direction == AiSearchBoostDirection::Exists) {
                        totals[index] += 1.0;
                    }
                }
            }
            AiSearchBoostDirection::Asc | AiSearchBoostDirection::Desc => {
                let mut ranked = values
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| value.as_ref().map(|value| (index, value)))
                    .collect::<Vec<_>>();
                ranked.sort_by(|left, right| {
                    left.1.compare(right.1).then_with(|| {
                        candidates[left.0]
                            .chunk_id
                            .cmp(&candidates[right.0].chunk_id)
                    })
                });
                if direction == AiSearchBoostDirection::Desc {
                    ranked.reverse();
                }
                let divisor = ranked.len().saturating_sub(1).max(1) as f32;
                for (rank, (index, _)) in ranked.into_iter().enumerate() {
                    totals[index] += 1.0 - rank as f32 / divisor;
                }
            }
        }
    }
    let divisor = boosts.len() as f32;
    for (candidate, total) in candidates.iter_mut().zip(totals) {
        let boost = MAX_METADATA_BOOST * total / divisor;
        candidate.boosting_score = Some(boost);
        candidate.public_score = candidate.retrieval_score + boost;
    }
    let maximum = candidates
        .iter()
        .map(|candidate| candidate.public_score)
        .fold(0.0_f32, f32::max);
    if maximum > 0.0 {
        for candidate in candidates.iter_mut() {
            candidate.public_score /= maximum;
        }
    }
    candidates.sort_by(|left, right| {
        right
            .public_score
            .total_cmp(&left.public_score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    Ok(())
}

enum BoostValue {
    Number(f64),
    Datetime(jiff::Timestamp),
    Present,
}

impl BoostValue {
    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.total_cmp(right),
            (Self::Datetime(left), Self::Datetime(right)) => left.cmp(right),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

fn boost_value(
    chunk: &AiSearchChunkRecord,
    field: &str,
    field_type: AiSearchMetadataType,
) -> Result<Option<BoostValue>, PlatformError> {
    if field.eq_ignore_ascii_case("timestamp") {
        return Ok(Some(BoostValue::Datetime(
            jiff::Timestamp::from_millisecond(chunk.item_created_at_ms).map_err(|_| corrupt())?,
        )));
    }
    let metadata: Map<String, Value> =
        serde_json::from_slice(&chunk.metadata_json).map_err(|_| corrupt())?;
    let Some(value) = metadata
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field))
        .map(|(_, value)| value)
    else {
        return Ok(None);
    };
    match field_type {
        AiSearchMetadataType::Number => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(BoostValue::Number)
            .map(Some)
            .ok_or_else(corrupt),
        AiSearchMetadataType::Datetime => value
            .as_str()
            .ok_or_else(corrupt)?
            .parse::<jiff::Timestamp>()
            .map(BoostValue::Datetime)
            .map(Some)
            .map_err(|_| corrupt()),
        AiSearchMetadataType::Text | AiSearchMetadataType::Boolean => Ok(Some(BoostValue::Present)),
    }
}

fn metadata_matches(chunk: &AiSearchChunkRecord, filter: Option<&FilterExpr>) -> bool {
    filter.is_none_or(|filter| {
        serde_json::from_slice::<Value>(&chunk.metadata_json)
            .ok()
            .and_then(|metadata| validate_metadata(&metadata).ok())
            .is_some_and(|metadata| filter.matches(&metadata))
    })
}

fn sort_ranked(candidates: &mut [RankedCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
}

#[cfg(test)]
mod boosting_tests {
    use super::*;
    use open_compute_search::ai_search::ScoredCandidate;

    fn chunk(id: &str, metadata: &Value, timestamp: i64) -> AiSearchChunkRecord {
        AiSearchChunkRecord {
            id: id.to_owned(),
            item_id: format!("item-{id}"),
            ordinal: 0,
            start_byte: 0,
            end_byte: 1,
            text: id.to_owned(),
            embedding: None,
            metadata_json: serde_json::to_vec(&metadata).unwrap(),
            item_key: format!("{id}.txt"),
            item_created_at_ms: timestamp,
        }
    }

    fn candidate(id: &str, score: f32) -> ScoredCandidate {
        ScoredCandidate {
            chunk_id: id.to_owned(),
            retrieval_score: score,
            boosting_score: None,
            reranking_score: None,
            public_score: score,
            vector_rank: None,
            vector_score: None,
            keyword_rank: None,
            keyword_score: None,
        }
    }

    #[test]
    fn metadata_boosting_is_case_insensitive_additive_and_deterministic() {
        let first = chunk("first", &json!({"Priority": 1, "draft": true}), 1);
        let second = chunk("second", &json!({"priority": 9}), 2);
        let chunks = HashMap::from([(first.id.as_str(), &first), (second.id.as_str(), &second)]);
        let fields = vec![
            crate::ai_search_config::AiSearchMetadataField {
                field_name: "priority".to_owned(),
                data_type: AiSearchMetadataType::Number,
            },
            crate::ai_search_config::AiSearchMetadataField {
                field_name: "draft".to_owned(),
                data_type: AiSearchMetadataType::Boolean,
            },
        ];
        let mut candidates = vec![candidate("first", 0.4), candidate("second", 0.3)];
        apply_boosting(
            &mut candidates,
            &chunks,
            &[
                AiSearchBoost {
                    field: "PRIORITY".to_owned(),
                    direction: Some(AiSearchBoostDirection::Desc),
                },
                AiSearchBoost {
                    field: "draft".to_owned(),
                    direction: Some(AiSearchBoostDirection::NotExists),
                },
            ],
            &fields,
        )
        .unwrap();
        assert_eq!(candidates[0].chunk_id, "second");
        assert_eq!(candidates[0].boosting_score, Some(0.3));
        assert_eq!(candidates[0].public_score, 1.0);
        assert!((candidates[1].public_score - 2.0 / 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn metadata_boosting_matches_hosted_normalized_score_projection() {
        let low = chunk("low", &json!({"priority": 1}), 1);
        let middle = chunk("middle", &json!({"priority": 2}), 1);
        let high = chunk("high", &json!({"priority": 3}), 1);
        let chunks = HashMap::from([
            (low.id.as_str(), &low),
            (middle.id.as_str(), &middle),
            (high.id.as_str(), &high),
        ]);
        let fields = vec![crate::ai_search_config::AiSearchMetadataField {
            field_name: "priority".to_owned(),
            data_type: AiSearchMetadataType::Number,
        }];
        let mut candidates = vec![
            candidate("low", 1.0),
            candidate("middle", 1.0),
            candidate("high", 1.0),
        ];
        apply_boosting(
            &mut candidates,
            &chunks,
            &[AiSearchBoost {
                field: "priority".to_owned(),
                direction: Some(AiSearchBoostDirection::Desc),
            }],
            &fields,
        )
        .unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["high", "middle", "low"]
        );
        for (candidate, expected) in candidates.iter().zip([1.0, 1.15 / 1.3, 1.0 / 1.3]) {
            assert!((candidate.public_score - expected).abs() < 1e-6);
        }
    }
}
