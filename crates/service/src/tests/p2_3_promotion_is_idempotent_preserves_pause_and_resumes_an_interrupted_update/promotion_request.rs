use super::*;

pub(super) struct Target {
    pub(super) account: open_compute_core::InstanceId,
    pub(super) worker: open_compute_core::WorkerId,
    pub(super) queue: open_compute_core::QueueId,
}

pub(super) fn build(
    target: &Target,
    key: &str,
    label: &str,
    promote: bool,
    cron: &str,
    batch_size: u32,
) -> CreateVersionRequest {
    let source = format!(
        "export default {{ fetch() {{ return new Response('{label}'); }}, queue() {{}}, scheduled() {{}} }};"
    );
    let bundle = CanonicalBundle::build(
        "index.js",
        vec![ModuleInput {
            name: "index.js".to_owned(),
            module_type: ModuleType::EsModule,
            bytes: source.into_bytes(),
        }],
        BundleLimits::default(),
    )
    .unwrap();
    CreateVersionRequest {
        instance_id: target.account,
        worker_id: target.worker,
        idempotency_key: key.to_owned(),
        content: open_compute_workers::VersionContent::Worker {
            bundle: bundle.into_bytes().into(),
            assets: None,
        },
        vars: std::collections::BTreeMap::new(),
        secrets: std::collections::BTreeMap::new(),
        bindings: std::collections::BTreeMap::new(),
        services: std::collections::BTreeMap::new(),
        runtime_features: open_compute_workers::VersionRuntimeFeatures {
            compatibility_date: "2026-09-08".to_owned(),
            ..Default::default()
        },
        queue_consumers: vec![QueueConsumerInput {
            queue: target.queue,
            entrypoint: None,
            config: open_compute_storage::queue_consumers::QueueConsumerConfig {
                max_batch_size: batch_size,
                ..open_compute_storage::queue_consumers::QueueConsumerConfig::default()
            },
            dead_letter_queue: None,
        }],
        crons: vec![cron.to_owned()],
        deployment_source: promote
            .then_some(open_compute_storage::worker_repository::DeploymentSource::VersionsApi),
        observability: None,
        request_id: open_compute_core::RequestId::generate(),
        now_ms: 60_000,
    }
}

pub(super) async fn remove_all_products(
    controller: &VersionController<'_>,
    target: &Target,
    storage: &Arc<open_compute_storage::PlatformStorage>,
    scheduler_path: &Path,
) {
    let mut request = build(target, "p23-empty", "empty", true, "ignored", 10);
    request.queue_consumers.clear();
    request.crons = Vec::new();
    let emptied_id = match controller.create_version(request).await.unwrap() {
        CreateVersionOutcome::Applied(result) => result.version.id,
        CreateVersionOutcome::Replay(_) => panic!("empty P2.3 version replayed"),
    };
    let workers = open_compute_storage::worker_repository::WorkerRepository::new(storage.db());
    assert_eq!(
        workers
            .get_worker(target.account, target.worker)
            .unwrap()
            .active_version_id,
        Some(emptied_id)
    );
    assert!(
        open_compute_storage::queue_consumers::QueueConsumerRepository::new(storage.db())
            .live_for_queue(target.queue)
            .unwrap()
            .is_none()
    );
    assert!(
        open_compute_storage::cron::CronRepository::new(storage.db())
            .live_for_worker(target.worker)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        open_compute_storage::scheduler::inspect_p23_cross_database(
            &storage.data_dir().control_db_path(),
            scheduler_path,
            100,
        )
        .unwrap(),
        open_compute_storage::scheduler::P23CrossDatabaseInspection::default()
    );
}
