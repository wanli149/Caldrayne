use crate::web::ui::api::UiRequestSender;
use core::{future::Future, ops::Deref};
use prometheus::Registry;
use server::chat::ChatCache;
use std::{future::IntoFuture, io, net::SocketAddr};

mod chat;
mod health;
mod routes;
mod ui;

pub(crate) use health::set_chunk_lifecycle_observability_status;
#[cfg(feature = "worldgen")]
pub(crate) use health::set_world_compat_observability_status;
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

#[cfg(test)]
pub(crate) struct SmokeHttpResponse {
    pub status_line: String,
    pub body: String,
}

#[cfg(test)]
pub(crate) async fn smoke_http_get(
    bind_address: SocketAddr,
    path: &str,
) -> io::Result<SmokeHttpResponse> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(bind_address).await?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {bind_address}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await?;

    let response = String::from_utf8(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("smoke response was not valid UTF-8: {error}"),
        )
    })?;
    let mut sections = response.splitn(2, "\r\n\r\n");
    let headers = sections.next().unwrap_or_default();
    let body = sections.next().unwrap_or_default().to_owned();
    let status_line = headers.lines().next().unwrap_or_default().to_owned();

    Ok(SmokeHttpResponse { status_line, body })
}
