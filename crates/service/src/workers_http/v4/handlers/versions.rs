use super::*;
use axum::Json;
use axum::body::to_bytes;
use axum::response::IntoResponse as _;
use open_compute_storage::worker_repository::{VersionContentKind, VersionState};

#[derive(Serialize)]
struct DeleteVersionResponse {
    errors: [serde_json::Value; 0],
    messages: [serde_json::Value; 0],
    success: bool,
}

pub(super) async fn list_beta_versions(
    State(state): State<HttpState>,
    Path((account, identifier)): Path<(String, String)>,
    request: Request,
) -> axum::response::Response {
    let context = match authorize(&request, V4Permission::Read) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let result = (|| {
        let query = query::version_list(request.uri().query(), false)?;
        let account = domain::resolve_instance(&state, &account)?;
        let api = worker_api(&state)?;
        let authority = state.v4_instance_context().ok_or(V4Error::Unavailable)?;
        let repository = WorkerRepository::new(api.storage.db());
        let worker = worker_by_identifier(repository, authority, account, &identifier)
            .map_err(|error| V4Error::from(&error))?;
        let versions = repository
            .list_versions(account, worker.id)
            .map_err(|error| V4Error::from(&error))?
            .into_iter()
            .filter(|version| version.deleted_at_ms.is_none())
            .collect::<Vec<_>>();
        let total = versions.len();
        let items = versions
            .iter()
            .skip(query.page.saturating_sub(1).saturating_mul(query.per_page))
            .take(query.per_page)
            .map(|version| {
                let snapshot = repository
                    .version_snapshot(account, worker.id, version.id, false)
                    .map_err(|error| V4Error::from(&error))?;
                beta_version_item(api, authority, &snapshot)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let info = V4ResultInfo {
            page: query.page,
            per_page: query.per_page,
            count: items.len(),
            total_count: total,
            total_pages: total.div_ceil(query.per_page),
        };
        Ok((items, info))
    })();
    match result {
        Ok((items, info)) => paginated_response(context, items, info),
        Err(error) => error_response(error, context.request_id()),
    }
}

pub(super) async fn get_beta_version(
    State(state): State<HttpState>,
    Path((account, identifier, requested)): Path<(String, String, String)>,
    request: Request,
) -> axum::response::Response {
    let context = match authorize(&request, V4Permission::Read) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let result = async move {
        let include =
            url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes())
                .into_owned()
                .collect::<Vec<_>>();
        let include_modules = match include.as_slice() {
            [] => false,
            [(key, value)] if key == "include" && value == "modules" => true,
            _ => return Err(V4Error::InvalidRequest),
        };
        let account = domain::resolve_instance(&state, &account)?;
        let api = worker_api(&state)?;
        let authority = state.v4_instance_context().ok_or(V4Error::Unavailable)?;
        let repository = WorkerRepository::new(api.storage.db());
        let worker = worker_by_identifier(repository, authority, account, &identifier)
            .map_err(|error| V4Error::from(&error))?;
        let version = resolve_version(repository, account, worker.id, &requested)?;
        if version.deleted_at_ms.is_some() {
            return Err(V4Error::NotFound);
        }
        let snapshot = repository
            .version_snapshot(account, worker.id, version.id, false)
            .map_err(|error| V4Error::from(&error))?;
        let mut item = beta_version_item(api, authority, &snapshot)?;
        if include_modules {
            item["modules"] = beta_version_modules(api, &version).await?;
        }
        Ok(item)
    }
    .await;
    respond(context, result)
}

fn beta_version_item(
    api: &crate::workers_http::WorkerApiState,
    authority: &crate::cloudflare_v4::accounts::V4InstanceContext,
    snapshot: &VersionSnapshot,
) -> Result<serde_json::Value, V4Error> {
    let version = &snapshot.version;
    let mut item = serde_json::json!({
        "id":version.id, "number":version.version_number, "urls":[],
        "created_on":crate::cloudflare_v4::iso_timestamp(version.created_at_ms)?,
        "source":"open-compute", "annotations":snapshot.annotations,
        "compatibility_date":version.compatibility_date,
        "compatibility_flags":version.compatibility_flags,
        "bindings":super::super::projection::public_bindings(api, authority, snapshot)
            .map_err(|error| V4Error::from(&error))?,
        "limits":{"cpu_ms":version.resource_limits.cpu_ms,"subrequests":version.resource_limits.sub_requests}
    });
    if let Some(main) = &version.main_module {
        item["main_module"] = serde_json::json!(main);
    }
    Ok(item)
}

async fn beta_version_modules(
    api: &crate::workers_http::WorkerApiState,
    version: &VersionRecord,
) -> Result<serde_json::Value, V4Error> {
    use base64::Engine as _;
    let (digest, size) = match (
        version.content_kind,
        version.artifact_sha256,
        version.artifact_size,
    ) {
        (VersionContentKind::AssetsOnly, None, None) => return Ok(serde_json::json!([])),
        (VersionContentKind::Worker, Some(digest), Some(size)) => (digest, size),
        _ => return Err(V4Error::IntegrityFailure),
    };
    let artifact = open_compute_artifacts::ArtifactRef::new(
        open_compute_artifacts::ARTIFACT_KEY_VERSION,
        &hex::encode(digest),
        size,
    )
    .map_err(|error| V4Error::from(&error))?;
    let bytes = api
        .artifacts
        .open(&artifact)
        .await
        .map_err(|error| V4Error::from(&error))?;
    let bundle = open_compute_workers::CanonicalBundle::parse(bytes.to_vec(), api.bundle_limits)
        .map_err(|error| V4Error::from(&error))?;
    let modules = bundle
        .manifest()
        .modules
        .iter()
        .map(|module| {
            let bytes = bundle
                .module_bytes(module)
                .map_err(|error| V4Error::from(&error))?;
            Ok(serde_json::json!({"name":module.name,
            "content_type":super::super::download::module_content_type(module.module_type),
            "content_base64":base64::engine::general_purpose::STANDARD.encode(bytes)}))
        })
        .collect::<Result<Vec<_>, V4Error>>()?;
    Ok(serde_json::json!(modules))
}

pub(super) async fn delete_beta_worker(
    State(state): State<HttpState>,
    Path((account, identifier)): Path<(String, String)>,
    request: Request,
) -> axum::response::Response {
    let context = match authorize(&request, V4Permission::ProductWrite) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let resolved = (|| {
        let instance = domain::resolve_instance(&state, &account)?;
        let api = worker_api(&state)?;
        let authority = state.v4_instance_context().ok_or(V4Error::Unavailable)?;
        worker_by_identifier(
            WorkerRepository::new(api.storage.db()),
            authority,
            instance,
            &identifier,
        )
        .map_err(|error| V4Error::from(&error))
    })();
    match resolved {
        Ok(worker) => {
            super::super::mutations::delete_script(
                State(state),
                Path((account, worker.name)),
                request,
            )
            .await
        }
        Err(error) => error_response(error, context.request_id()),
    }
}

pub(super) async fn get_beta_worker(
    State(state): State<HttpState>,
    Path((account, worker)): Path<(String, String)>,
    request: Request,
) -> axum::response::Response {
    let context = match authorize(&request, V4Permission::Read) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    if request.uri().query().is_some() {
        return error_response(V4Error::InvalidRequest, context.request_id());
    }
    let account = match domain::resolve_instance(&state, &account) {
        Ok(value) => value,
        Err(error) => return error_response(error, context.request_id()),
    };
    let api = match worker_api(&state) {
        Ok(value) => value,
        Err(error) => return error_response(error, context.request_id()),
    };
    let Some(authority) = state.v4_instance_context() else {
        return error_response(V4Error::Unavailable, context.request_id());
    };
    let worker = match worker_by_identifier(
        WorkerRepository::new(api.storage.db()),
        authority,
        account,
        &worker,
    ) {
        Ok(value) => value,
        Err(error) => return platform_error(context.request_id(), &error),
    };
    let created_on = match crate::cloudflare_v4::iso_timestamp(worker.created_at_ms) {
        Ok(value) => value,
        Err(error) => return error_response(error, context.request_id()),
    };
    let updated_on = match crate::cloudflare_v4::iso_timestamp(worker.updated_at_ms) {
        Ok(value) => value,
        Err(error) => return error_response(error, context.request_id()),
    };
    success_response(
        context,
        serde_json::json!({
            "id": authority.public_worker_tag(worker.id),
            "name": worker.name,
            "tags": [],
            "subdomain": { "enabled": false, "previews_enabled": false },
            "observability": {},
            "logpush": false,
            "tail_consumers": [],
            "created_on": created_on,
            "updated_on": updated_on,
            "references": {
                "workers": [],
                "domains": [],
                "dispatch_namespace_outbounds": [],
                "durable_objects": [],
                "queues": [],
            },
        }),
    )
}

pub(super) async fn delete_beta_version(
    State(state): State<HttpState>,
    Path((account, worker, version)): Path<(String, String, String)>,
    request: Request,
) -> axum::response::Response {
    let context = match authorize(&request, V4Permission::ProductWrite) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    if request.uri().query().is_some()
        || request
            .headers()
            .contains_key(axum::http::header::CONTENT_TYPE)
    {
        return error_response(V4Error::InvalidRequest, context.request_id());
    }
    match to_bytes(request.into_body(), 1).await {
        Ok(bytes) if bytes.is_empty() => {}
        _ => return error_response(V4Error::InvalidRequest, context.request_id()),
    }
    let account = match domain::resolve_instance(&state, &account) {
        Ok(value) => value,
        Err(error) => return error_response(error, context.request_id()),
    };
    let api = match worker_api(&state) {
        Ok(value) => value,
        Err(error) => return error_response(error, context.request_id()),
    };
    let Some(authority) = state.v4_instance_context() else {
        return error_response(V4Error::Unavailable, context.request_id());
    };
    let repository = WorkerRepository::new(api.storage.db());
    let worker = match worker_by_identifier(repository, authority, account, &worker) {
        Ok(value) => value,
        Err(error) => return platform_error(context.request_id(), &error),
    };
    let version = match resolve_version(repository, account, worker.id, &version) {
        Ok(value) => value,
        Err(error) => return error_response(error, context.request_id()),
    };
    if version.state == VersionState::Tombstoned {
        return deleted();
    }
    let storage = api.storage.clone();
    let worker_id = worker.id;
    let version_id = version.id;
    let begin = tokio::task::spawn_blocking(move || {
        WorkerRepository::new(storage.db()).begin_version_delete(account, worker_id, version_id)
    })
    .await;
    match begin {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return platform_error(context.request_id(), &error);
        }
        Err(_) => return error_response(V4Error::Internal, context.request_id()),
    }
    if let Err(error) = api
        .pins
        .fence_and_wait(version_id, api.delete_drain_timeout)
        .await
    {
        return platform_error(context.request_id(), &error);
    }
    let storage = api.storage.clone();
    let finish = tokio::task::spawn_blocking(move || {
        WorkerRepository::new(storage.db()).finalize_version_delete(
            account,
            worker_id,
            version_id,
            context.request_id(),
            now_ms(),
        )
    })
    .await;
    match finish {
        Ok(Ok(())) => {
            api.pins.retire_fence(version_id);
            deleted()
        }
        Ok(Err(error)) => platform_error(context.request_id(), &error),
        Err(_) => error_response(V4Error::Internal, context.request_id()),
    }
}

fn worker_by_identifier(
    repository: WorkerRepository<'_>,
    authority: &crate::cloudflare_v4::accounts::V4InstanceContext,
    account: open_compute_core::InstanceId,
    requested: &str,
) -> Result<WorkerRecord, PlatformError> {
    repository.list_workers(account).and_then(|workers| {
        workers
            .into_iter()
            .find(|worker| {
                worker.name == requested
                    || authority.matches_public_worker_tag(worker.id, requested)
            })
            .ok_or_else(|| {
                PlatformError::new(
                    open_compute_core::ErrorCode::WorkerNotFound,
                    "Worker was not found",
                )
            })
    })
}

fn resolve_version(
    repository: WorkerRepository<'_>,
    account: open_compute_core::InstanceId,
    worker: open_compute_core::WorkerId,
    requested: &str,
) -> Result<VersionRecord, V4Error> {
    let versions = repository
        .list_versions(account, worker)
        .map_err(|error| V4Error::from(&error))?;
    if requested == "latest" {
        return versions
            .into_iter()
            .find(|version| version.deleted_at_ms.is_none())
            .ok_or(V4Error::NotFound);
    }
    if let Ok(id) = VersionId::from_str(requested) {
        return versions
            .into_iter()
            .find(|version| version.id == id)
            .ok_or(V4Error::NotFound);
    }
    if requested.len() < 8
        || !requested
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(V4Error::InvalidRequest);
    }
    let mut matches = versions
        .into_iter()
        .filter(|version| version.id.to_string().starts_with(requested));
    let version = matches.next().ok_or(V4Error::NotFound)?;
    if matches.next().is_some() {
        return Err(V4Error::Conflict);
    }
    Ok(version)
}

fn deleted() -> axum::response::Response {
    Json(DeleteVersionResponse {
        errors: [],
        messages: [],
        success: true,
    })
    .into_response()
}

#[cfg(test)]
#[path = "versions_tests.rs"]
mod tests;
