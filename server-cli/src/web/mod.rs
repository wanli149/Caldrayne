use crate::web::ui::api::UiRequestSender;
use core::{future::Future, ops::Deref};
use prometheus::Registry;
use server::chat::ChatCache;
use std::{future::IntoFuture, io, net::SocketAddr};

mod chat;
mod health;
mod routes;
mod ui;

pub use health::{
    HealthState, RuntimeObservabilityInventory, RuntimeObservabilityState,
    RuntimeObservabilityStatus, default_runtime_observability_inventory,
    snapshot_runtime_observability_inventory,
};

pub async fn bind_listener<S>(addr: S) -> Result<tokio::net::TcpListener, std::io::Error>
where
    S: Into<SocketAddr>,
{
    tokio::net::TcpListener::bind(addr.into()).await
}

pub async fn run_with_listener<F, R>(
    registry: R,
    cache: ChatCache,
    chat_secret: Option<String>,
    ui_secret: String,
    web_ui_request_s: UiRequestSender,
    health_state: HealthState,
    listener: tokio::net::TcpListener,
    shutdown: F,
) -> io::Result<()>
where
    F: Future<Output = ()> + Send,
    R: Deref<Target = Registry> + Clone + Send + Sync + 'static,
{
    let app = routes::app_router(
        registry,
        cache,
        chat_secret,
        ui_secret,
        web_ui_request_s,
        health_state,
    );

    let bind_address = listener.local_addr().ok();
    if let Some(bind_address) = bind_address {
        tracing::info!(bind_address = %bind_address, "listening on web port");
    } else {
        tracing::info!("listening on web port");
    }
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .into_future();
    let res = tokio::select! {
        res = server => res,
        _ = shutdown => Ok(()),
    };
    res
}
