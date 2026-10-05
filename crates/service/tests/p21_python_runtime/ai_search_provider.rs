//! Bounded loopback `OpenAI` provider for the genuine daemon AI Search path.

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::IntoResponse as _;
use axum::routing::post;
use axum::{Json, Router};
use open_compute_core::{
    AiAuthConfig, AiBackendConfig, AiBackendProtocol, AiConfig, AiEmbeddingModelConfig,
    AiEmbeddingProfileConfig, AiGenerationCapability, AiGenerationModelConfig, AiTokenizer,
    AiTokenizerArtifactConfig, AiTokenizerConfig,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Default)]
struct Calls {
    embedding: AtomicUsize,
    chat: AtomicUsize,
    streaming: AtomicUsize,
}

pub(super) struct Provider {
    address: SocketAddr,
    calls: Arc<Calls>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Provider {
    pub(super) async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let calls = Arc::new(Calls::default());
        let state = calls.clone();
        let task = tokio::spawn(async move {
            let router = Router::new()
                .route("/embeddings", post(embeddings))
                .route("/chat/completions", post(chat))
                .layer(DefaultBodyLimit::max(128 * 1024))
                .with_state(state);
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            address,
            calls,
            stop,
            task,
        }
    }

    pub(super) fn config(&self) -> AiConfig {
        let tokenizer = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tokenizer-word-level.json")
            .canonicalize()
            .unwrap();
        let digest = hex::encode(Sha256::digest(std::fs::read(&tokenizer).unwrap()));
        let mut config = AiConfig::default();
        for (name, path, protocol) in [
            (
                "embedding",
                "embeddings",
                AiBackendProtocol::OpenAiEmbeddingsV1,
            ),
            (
                "chat",
                "chat/completions",
                AiBackendProtocol::OpenAiChatCompletionsV1,
            ),
        ] {
            config.backends.insert(
                name.to_owned(),
                AiBackendConfig {
                    protocol,
                    endpoint: format!("http://{}/{path}", self.address),
                    auth: AiAuthConfig::None,
                    headers: BTreeMap::new(),
                },
            );
        }
        config.embedding_profiles.insert(
            "fixture/profile".to_owned(),
            AiEmbeddingProfileConfig {
                dimensions: 3,
                max_input_tokens: 4096,
                send_dimensions: false,
                tokenizer: AiTokenizerConfig {
                    kind: AiTokenizer::Custom,
                    revision: "word-level-fixture".to_owned(),
                    artifact: AiTokenizerArtifactConfig {
                        path: tokenizer,
                        sha256: digest,
                    },
                },
            },
        );
        config.embedding_models.insert(
            "fixture/embedding".to_owned(),
            AiEmbeddingModelConfig {
                backend: "embedding".to_owned(),
                remote_model: "fixture/embedding".to_owned(),
                provider_revision: Some("fixture-1".to_owned()),
                profile: "fixture/profile".to_owned(),
            },
        );
        config.default_embedding_model = Some("fixture/embedding".to_owned());
        config.generation_models.insert(
            "fixture/chat".to_owned(),
            AiGenerationModelConfig {
                backend: "chat".to_owned(),
                remote_model: "fixture/chat".to_owned(),
                provider_revision: Some("fixture-1".to_owned()),
                max_context_tokens: 4096,
                capabilities: BTreeSet::from([AiGenerationCapability::Chat]),
            },
        );
        config.default_generation_model = Some("fixture/chat".to_owned());
        config.validate().unwrap();
        config
    }

    pub(super) async fn finish(self) {
        assert!(self.calls.embedding.load(Ordering::Relaxed) > 0);
        assert!(self.calls.chat.load(Ordering::Relaxed) > 0);
        assert!(self.calls.streaming.load(Ordering::Relaxed) > 0);
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
        assert!(tokio::net::TcpListener::bind(self.address).await.is_ok());
    }
}

async fn embeddings(
    State(calls): State<Arc<Calls>>,
    Json(input): Json<Value>,
) -> axum::response::Response {
    if input["model"] != "fixture/embedding" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(values) = input["input"].as_array() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if values.is_empty()
        || values.len() > 96
        || values
            .iter()
            .any(|v| v.as_str().is_none_or(|s| s.len() > 65536))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    calls.embedding.fetch_add(1, Ordering::Relaxed);
    let data: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let digest = Sha256::digest(value.as_str().unwrap().as_bytes());
            let embedding = [
                f32::from(digest[0]) + 1.0,
                f32::from(digest[1]) + 1.0,
                f32::from(digest[2]) + 1.0,
            ];
            json!({"object":"embedding","index":index,"embedding":embedding})
        })
        .collect();
    Json(json!({"object":"list","model":"fixture/embedding","data":data,"usage":{"prompt_tokens":values.len(),"total_tokens":values.len()}})).into_response()
}

async fn chat(
    State(calls): State<Arc<Calls>>,
    Json(input): Json<Value>,
) -> axum::response::Response {
    if input["model"] != "fixture/chat"
        || !input["messages"].as_array().is_some_and(|v| !v.is_empty())
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    calls.chat.fetch_add(1, Ordering::Relaxed);
    if input["stream"] == true {
        calls.streaming.fetch_add(1, Ordering::Relaxed);
        return (
            [("content-type", "text/event-stream")],
            concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"native \"}}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"answer\"}}]}\n\n",
                "data: [DONE]\n\n"
            ),
        )
            .into_response();
    }
    Json(json!({"model":"fixture/chat","choices":[{"index":0,"message":{"role":"assistant","content":"native answer"},"finish_reason":"stop"}]})).into_response()
}
