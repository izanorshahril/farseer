//! The shell's own local server: the canvas, and a proxy to farseer.
//!
//! The webview loads **one origin**, this one. `/v1` is proxied to farseer with
//! the operator token attached here, so the token stays on the Rust side and the
//! page never holds it - the same property the Vite dev proxy has, which
//! `28 operator surface` gate 3 requires and which `localStorage` would lose.
//!
//! This is not `01 cell primitive`'s ruling being walked back. The *runtime*
//! still serves no HTML and knows nothing about widgets; the shell is a client
//! of `/v1` like any other, and it happens to also hand the page to its own
//! webview.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use tower_http::services::{ServeDir, ServeFile};

pub struct Shell {
    pub farseer: String,
    pub token: String,
    pub client: reqwest::Client,
    /// Where the cell definitions the operator edits actually live.
    pub cells: PathBuf,
}

/// Serve the canvas and proxy `/v1`, on a port the OS chooses.
///
/// Returns the bound address, because the shell has to tell its own webview
/// where to look and guessing a port is how two instances collide.
pub async fn start(
    canvas: PathBuf,
    cells: PathBuf,
    farseer_port: u16,
    token: String,
) -> Result<String> {
    let state = Arc::new(Shell {
        farseer: format!("http://127.0.0.1:{farseer_port}"),
        token,
        client: reqwest::Client::new(),
        cells,
    });

    // Any unknown path falls back to the canvas entry point, because the page
    // owns its own routing and a hard reload on a sub-path must not 404.
    let index = canvas.join("index.html");
    let app = Router::new()
        .route("/v1/{*rest}", any(proxy))
        .route("/v1", any(proxy))
        .route("/__settings/runners", get(list_runners))
        .route(
            "/__settings/top-manager",
            get(read_top_manager).put(write_top_manager),
        )
        .fallback_service(ServeDir::new(canvas).fallback(ServeFile::new(index)))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("shell server stopped: {error}");
        }
    });
    Ok(format!("http://127.0.0.1:{}", addr.port()))
}

/// What this machine could actually run, per `10 runner inventory`'s rule that
/// reach is observed rather than advertised.
async fn list_runners() -> Json<Vec<crate::settings::RunnerChoice>> {
    Json(crate::settings::runners())
}

async fn read_top_manager(State(shell): State<Arc<Shell>>) -> Response {
    match crate::settings::top_manager(&shell.cells) {
        Ok(current) => Json(current).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// Write the definition, then ask the runtime to reload it.
///
/// The order is the decision: the file is the source of truth per `01 cell
/// primitive`, and reload is how the runtime finds out. A shell that updated
/// the runtime without the file would have created a second source that the
/// next restart silently discards.
async fn write_top_manager(
    State(shell): State<Arc<Shell>>,
    Json(request): Json<crate::settings::TopManagerRequest>,
) -> Response {
    let updated = match crate::settings::set_top_manager(&shell.cells, &request.runner) {
        Ok(updated) => updated,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    let reload = shell
        .client
        .post(format!("{}/v1/cells/reload", shell.farseer))
        .header(header::AUTHORIZATION, format!("Bearer {}", shell.token))
        .send()
        .await;
    match reload {
        // The runtime's own answer travels back untouched: it is the thing that
        // validates a definition, and the shell paraphrasing it would be a
        // second opinion with no authority.
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Json(serde_json::json!({
                "top_manager": updated,
                "reload_status": status.as_u16(),
                "reload": serde_json::from_str::<serde_json::Value>(&body)
                    .unwrap_or(serde_json::Value::String(body)),
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            format!("definition written, but farseer did not reload: {error}"),
        )
            .into_response(),
    }
}

/// Pass a request through to farseer, adding the credential the page does not have.
async fn proxy(State(shell): State<Arc<Shell>>, request: Request) -> Response {
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/v1");
    let url = format!("{}{path}", shell.farseer);
    let method = request.method().clone();
    let headers = request.headers().clone();

    let body = match axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };

    let mut forwarded = shell.client.request(method, &url).body(body);
    // Content type travels; hop-by-hop headers and any Authorization the page
    // tried to set do not. The token this proxy attaches is the only one.
    if let Some(value) = headers.get(header::CONTENT_TYPE) {
        forwarded = forwarded.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = headers.get("last-event-id") {
        forwarded = forwarded.header("last-event-id", value);
    }
    forwarded = forwarded.header(header::AUTHORIZATION, format!("Bearer {}", shell.token));

    match forwarded.send().await {
        Ok(response) => {
            let status = response.status();
            let mut builder = Response::builder().status(status);
            // Content type matters for SSE: without it the browser will not
            // treat `/v1/stream` as a stream at all.
            if let Some(value) = response.headers().get(header::CONTENT_TYPE) {
                builder = builder.header(header::CONTENT_TYPE, value);
            }
            builder = builder.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            builder
                .body(Body::from_stream(response.bytes_stream()))
                .unwrap_or_else(|error| {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                })
        }
        // A daemon that went away mid-session is worth naming: the canvas can
        // say so, and "connection refused" on a port nobody chose cannot.
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            format!("farseer is not answering: {error}"),
        )
            .into_response(),
    }
}

/// The canvas build output, next to the executable when installed and under the
/// repository during development.
pub fn canvas_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let beside = exe.parent()?.join("canvas");
    if beside.join("index.html").exists() {
        return Some(beside);
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join("ui")
        .join("dist");
    repo.join("index.html").exists().then_some(repo)
}
