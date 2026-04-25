use super::{
    HealthState, chat,
    health::{
        MetricsState, RuntimeObservabilityState, RuntimeObservabilitySurface,
        set_runtime_observability_status,
    },
    ui,
    ui::api::UiRequestSender,
};
use axum::{Json, Router, body::Bytes, extract::State, response::IntoResponse, routing::get};
use core::ops::Deref;
use http_body_util::Full;
use hyper::{StatusCode, header, http};
use prometheus::{Registry, TextEncoder};
use server::chat::ChatCache;

pub(super) fn app_router<R>(
    registry: R,
    cache: ChatCache,
    chat_secret: Option<String>,
    ui_secret: String,
    web_ui_request_s: UiRequestSender,
    health_state: HealthState,
) -> Router
where
    R: Deref<Target = Registry> + Clone + Send + Sync + 'static,
{
    let metrics = metrics_router(MetricsState {
        registry: registry.deref().clone(),
        contract: health_state.metrics_contract(),
        runtime_observability_inventory: health_state.runtime_observability_inventory.clone(),
    });
    let health = health_router(health_state);

    Router::new()
        .nest("/chat/v1", chat::router(cache, chat_secret))
        .nest(
            "/ui_api/v1",
            ui::api::router(web_ui_request_s, ui_secret.clone()),
        )
        .nest("/ui", ui::router(ui_secret))
        .nest("/metrics", metrics)
        .nest("/health", health)
}

fn metrics_router(metrics_state: MetricsState) -> Router {
    Router::new()
        .route("/", get(metrics))
        .route("/meta", get(metrics_meta))
        .with_state(metrics_state)
}

fn health_router(health_state: HealthState) -> Router {
    Router::new()
        .route("/", get(live))
        .route("/live", get(live))
        .route("/ready", get(ready))
        .route("/backup", get(backup))
        .route("/operations", get(health_operations))
        .route("/compatibility", get(health_compatibility))
        .route("/account-auth", get(health_account_auth))
        .route("/surfaces", get(health_surfaces))
        .route("/management-auth", get(health_management_auth))
        .route("/transport-security", get(health_transport_security))
        .route("/listeners", get(health_runtime_listeners))
        .route("/observability", get(health_runtime_observability))
        .route("/preflight", get(health_preflight))
        .route("/governance", get(health_governance))
        .route("/policy", get(health_policy))
        .route("/recovery", get(health_recovery))
        .route("/recovery/drill", get(health_recovery_drill))
        .with_state(health_state)
}

async fn live(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.liveness_report()),
    )
}

async fn ready(State(health): State<HealthState>) -> impl IntoResponse {
    let report = health.readiness_report();
    let status = if report.status == "ready" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(report))
}

async fn health_policy(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.health_contract()),
    )
}

async fn health_governance(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.governance_report()),
    )
}

async fn health_operations(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.operational_baseline_report()),
    )
}

async fn health_compatibility(State(health): State<HealthState>) -> impl IntoResponse {
    let report = health.compatibility_contract_report();
    let status = if report.status == "compatibility-contract-aligned" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(report))
}

async fn health_account_auth(State(health): State<HealthState>) -> impl IntoResponse {
    let report = health.account_auth_governance_report();
    let status = if report.startup_policy.startup_permitted {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(report))
}

async fn health_surfaces(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.surface_inventory_report()),
    )
}

async fn health_management_auth(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.management_auth_report()),
    )
}

async fn health_transport_security(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.transport_security_report()),
    )
}

async fn health_runtime_listeners(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.runtime_listener_report()),
    )
}

async fn health_runtime_observability(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.runtime_observability_report()),
    )
}

async fn health_preflight(State(health): State<HealthState>) -> impl IntoResponse {
    let report = health.preflight_report();
    let status = if report.release_blocked {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(report))
}

async fn health_recovery(State(health): State<HealthState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(health.recovery_contract()),
    )
}

async fn health_recovery_drill(State(health): State<HealthState>) -> impl IntoResponse {
    let report = health.recovery_drill_report();
    let status = if report.status == "drill_ready" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(report))
}

async fn backup(State(health): State<HealthState>) -> impl IntoResponse {
    let report = health.backup_report();
    let status = if report.status == "backup_ready" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(report))
}

async fn metrics_meta(State(metrics): State<MetricsState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(metrics.contract),
    )
}

async fn metrics(State(metrics): State<MetricsState>) -> Result<impl IntoResponse, StatusCode> {
    use prometheus::Encoder;

    let mf = metrics.registry.gather();
    let mut buffer = Vec::with_capacity(1024);

    let encoder = TextEncoder::new();
    encoder
        .encode(&mf, &mut buffer)
        .expect("write to vec cannot fail");

    let bytes: Bytes = buffer.into();

    match http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, encoder.format_type())
        .header(header::CACHE_CONTROL, "no-store")
        .body(Full::new(bytes))
    {
        Err(e) => {
            set_runtime_observability_status(
                &metrics.runtime_observability_inventory,
                RuntimeObservabilitySurface::MetricsExport,
                RuntimeObservabilityState::Failing,
                format!("failed to encode metrics HTTP response: {e}"),
            );
            tracing::warn!(?e, "could not export metrics to HTTP format");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        },
        Ok(r) => {
            set_runtime_observability_status(
                &metrics.runtime_observability_inventory,
                RuntimeObservabilitySurface::MetricsExport,
                RuntimeObservabilityState::Healthy,
                "no metrics export failures observed since startup",
            );
            Ok(r)
        },
    }
}
