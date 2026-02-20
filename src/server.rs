//! HTTP API server: axum router and request handlers.
//!
//! The server runs on the tokio async runtime while the render thread
//! runs on a plain `std::thread`. Communication is via `std::sync::mpsc`.
//!
//! ## Rust concepts
//! - `async fn` and `.await` for non-blocking I/O
//! - axum extractors: `State`, `Json`, `Bytes`
//! - `Arc` for sharing state across async tasks
//! - Serde `Deserialize` for parsing JSON request bodies
//! - `tower-http` middleware for CORS

use crate::PanelConfig;
use crate::media::{self, MediaEntry, VideoEntry};
use crate::render::{DisplayState, DisplayStatus, RenderCommand};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

// ── App State ────────────────────────────────────────────────────────

/// Shared application state, passed to every handler via axum's `State` extractor.
///
/// Rust concept: CLONE for Arc
/// `Arc` (Atomic Reference Counting) is cheap to clone — it just increments
/// a counter. axum clones the state for each request handler, so everything
/// inside must be cheaply cloneable. `Arc` makes that possible for shared data.
#[derive(Clone)]
pub struct AppState {
    /// Channel to send commands to the render thread
    pub command_tx: Sender<RenderCommand>,
    /// Shared display status (render thread writes, handlers read)
    pub status: Arc<Mutex<DisplayStatus>>,
    /// Root directory for media files (images/, videos/)
    pub media_dir: PathBuf,
    /// Panel dimensions
    pub panel: PanelConfig,
}

// ── OpenAPI Documentation ────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
    paths(
        get_status,
        get_images,
        get_videos,
        get_fonts,
        post_display_image,
        post_display_video,
        post_display_text,
        post_display_clear,
        post_display_stop,
        post_brightness,
    ),
    components(schemas(
        DisplayStatus,
        DisplayState,
        media::MediaEntry,
        media::VideoEntry,
        ImageRequest,
        VideoRequest,
        TextRequest,
        BrightnessRequest,
    )),
    tags(
        (name = "display", description = "Display control endpoints"),
        (name = "media", description = "Media discovery endpoints"),
        (name = "system", description = "System status endpoints"),
    ),
    info(
        title = "LED Matrix API",
        version = env!("CARGO_PKG_VERSION"),
        description = "HTTP API for controlling an RGB LED matrix"
    )
)]
pub struct ApiDoc;

// ── Request/Response types ───────────────────────────────────────────

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ImageRequest {
    /// Path to image file relative to media directory
    #[schema(example = "images/test.png")]
    path: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct VideoRequest {
    /// Path to video directory relative to media directory. Use GET /api/videos to list available videos.
    #[schema(example = "videos/eyes_25")]
    path: String,
    /// Frames per second. Typical range: 15-60. Higher fps = smoother but more CPU intensive.
    #[serde(default = "default_fps")]
    #[schema(example = 25, default = 30)]
    fps: u32,
    /// Loop playback indefinitely. Set to true to repeat video, false to play once and clear screen.
    #[serde(default, rename = "loop")]
    #[schema(example = true, default = false)]
    loop_playback: bool,
}

fn default_fps() -> u32 {
    30
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TextRequest {
    /// Text to display
    text: String,
    /// BDF font name. Available fonts: 4x6, 5x7, 5x8, 6x9, 6x10, 6x12, 6x13, 6x13B, 6x13O, 7x13, 7x13B, 7x13O, 7x14, 7x14B, 8x13, 8x13B, 8x13O, 9x15, 9x15B, 9x18, 9x18B, 10x20, and more in fonts/bdf/
    #[serde(default = "default_font")]
    #[schema(example = "6x13", default = "6x13")]
    font: String,
    /// RGB color array [red, green, blue] where each value is 0-255. Examples: [255, 0, 0] = red, [0, 255, 0] = green, [0, 0, 255] = blue, [255, 255, 255] = white
    #[serde(default = "default_color")]
    #[schema(value_type = Vec<u8>, example = "[255, 255, 255]")]
    color: (u8, u8, u8),
    /// Scroll speed in pixels per second. Typical range: 10-100
    #[serde(default = "default_speed")]
    #[schema(example = 30, default = 30)]
    speed: u32,
}

fn default_font() -> String {
    "6x13".to_string()
}

fn default_color() -> (u8, u8, u8) {
    (255, 255, 255)
}

fn default_speed() -> u32 {
    30
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct BrightnessRequest {
    /// Brightness level (0-100)
    #[schema(example = 75, minimum = 0, maximum = 100)]
    value: u8,
}

// ── Router ───────────────────────────────────────────────────────────

/// Build the axum router with all API endpoints.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .merge(
            SwaggerUi::new("/docs")
                .url("/api-docs/openapi.json", ApiDoc::openapi())
                .config(utoipa_swagger_ui::Config::new(["/api-docs/openapi.json"]).validator_url("none")),
        )
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/images", get(get_images))
        .route("/api/v1/videos", get(get_videos))
        .route("/api/v1/fonts", get(get_fonts))
        .route("/api/v1/display/image", post(post_display_image))
        .route("/api/v1/display/video", post(post_display_video))
        .route("/api/v1/display/text", post(post_display_text))
        .route("/api/v1/display/frame", post(post_display_frame))
        .route("/api/v1/display/stream", get(ws_display_stream))
        .route("/api/v1/display/clear", post(post_display_clear))
        .route("/api/v1/display/stop", post(post_display_stop))
        .route("/api/v1/brightness", post(post_brightness))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /api/v1/status — return current display state
#[utoipa::path(
    get,
    path = "/api/v1/status",
    tag = "system",
    responses(
        (status = 200, description = "Current display status", body = DisplayStatus)
    )
)]
async fn get_status(State(state): State<AppState>) -> Json<DisplayStatus> {
    let status = state.status.lock().unwrap().clone();
    Json(status)
}

/// GET /api/v1/images — list available images
#[utoipa::path(
    get,
    path = "/api/v1/images",
    tag = "media",
    responses(
        (status = 200, description = "List of available images", body = Vec<MediaEntry>)
    )
)]
async fn get_images(State(state): State<AppState>) -> Json<Vec<media::MediaEntry>> {
    let images = media::list_images(&state.media_dir);
    Json(images)
}

/// GET /api/v1/videos — list available video directories
#[utoipa::path(
    get,
    path = "/api/v1/videos",
    tag = "media",
    responses(
        (status = 200, description = "List of available videos", body = Vec<VideoEntry>)
    )
)]
async fn get_videos(State(state): State<AppState>) -> Json<Vec<media::VideoEntry>> {
    let videos = media::list_videos(&state.media_dir);
    Json(videos)
}

/// GET /api/v1/fonts — list available BDF fonts
#[utoipa::path(
    get,
    path = "/api/v1/fonts",
    tag = "media",
    responses(
        (status = 200, description = "List of available font names", body = Vec<String>)
    )
)]
async fn get_fonts(State(state): State<AppState>) -> Json<Vec<String>> {
    let fonts = media::list_fonts(&state.media_dir);
    Json(fonts)
}

/// POST /api/v1/display/image — display a static image
#[utoipa::path(
    post,
    path = "/api/v1/display/image",
    tag = "display",
    request_body = ImageRequest,
    responses(
        (status = 200, description = "Image displayed successfully"),
        (status = 404, description = "Image not found"),
        (status = 400, description = "Invalid path")
    )
)]
async fn post_display_image(
    State(state): State<AppState>,
    Json(req): Json<ImageRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let full_path = validate_media_path(&state.media_dir, &req.path)?;

    state
        .command_tx
        .send(RenderCommand::ShowImage(full_path))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Render thread gone".to_string(),
            )
        })?;

    Ok(StatusCode::OK)
}

/// POST /api/v1/display/video — play a video (directory of frame images)
#[utoipa::path(
    post,
    path = "/api/v1/display/video",
    tag = "display",
    request_body = VideoRequest,
    responses(
        (status = 200, description = "Video playback started"),
        (status = 404, description = "Video directory not found"),
        (status = 400, description = "Invalid path")
    )
)]
async fn post_display_video(
    State(state): State<AppState>,
    Json(req): Json<VideoRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let full_path = validate_media_path(&state.media_dir, &req.path)?;

    state
        .command_tx
        .send(RenderCommand::PlayVideo {
            dir: full_path,
            fps: req.fps,
            loop_playback: req.loop_playback,
        })
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Render thread gone".to_string(),
            )
        })?;

    Ok(StatusCode::OK)
}

/// POST /api/v1/display/text — scroll text across the display
#[utoipa::path(
    post,
    path = "/api/v1/display/text",
    tag = "display",
    request_body = TextRequest,
    responses(
        (status = 200, description = "Text scrolling started"),
    )
)]
async fn post_display_text(
    State(state): State<AppState>,
    Json(req): Json<TextRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .command_tx
        .send(RenderCommand::ScrollText {
            text: req.text,
            font: req.font,
            color: req.color,
            speed: req.speed,
        })
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Render thread gone".to_string(),
            )
        })?;

    Ok(StatusCode::OK)
}

/// POST /api/v1/display/frame — push a raw RGB frame
///
/// Expects `application/octet-stream` body with exactly rows*cols*3 bytes.
async fn post_display_frame(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    let expected = state.panel.frame_byte_count();
    if body.len() != expected {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Expected {} bytes ({}x{}x3 RGB), got {} bytes",
                expected,
                state.panel.cols,
                state.panel.rows,
                body.len()
            ),
        ));
    }

    state
        .command_tx
        .send(RenderCommand::ShowFrame(body.to_vec()))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Render thread gone".to_string(),
            )
        })?;

    Ok(StatusCode::OK)
}

/// POST /api/v1/display/clear — clear the display
#[utoipa::path(
    post,
    path = "/api/v1/display/clear",
    tag = "display",
    responses(
        (status = 200, description = "Display cleared"),
    )
)]
async fn post_display_clear(
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.command_tx.send(RenderCommand::Clear).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Render thread gone".to_string(),
        )
    })?;

    Ok(StatusCode::OK)
}

/// POST /api/v1/display/stop — stop current playback
#[utoipa::path(
    post,
    path = "/api/v1/display/stop",
    tag = "display",
    responses(
        (status = 200, description = "Playback stopped"),
    )
)]
async fn post_display_stop(
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.command_tx.send(RenderCommand::Stop).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Render thread gone".to_string(),
        )
    })?;

    Ok(StatusCode::OK)
}

/// POST /api/v1/brightness — set display brightness (0-100)
#[utoipa::path(
    post,
    path = "/api/v1/brightness",
    tag = "display",
    request_body = BrightnessRequest,
    responses(
        (status = 200, description = "Brightness updated"),
    )
)]
async fn post_brightness(
    State(state): State<AppState>,
    Json(req): Json<BrightnessRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .command_tx
        .send(RenderCommand::SetBrightness(req.value))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Render thread gone".to_string(),
            )
        })?;

    Ok(StatusCode::OK)
}

// ── WebSocket streaming ─────────────────────────────────────────────

/// GET /api/v1/display/stream — WebSocket endpoint for streaming raw RGB frames.
///
/// Connect with a WebSocket client and send binary messages of exactly
/// rows*cols*3 bytes (RGB24). Each message is rendered as one frame.
/// Text messages are ignored. The connection sets status to `Streaming`
/// on connect and back to `Idle` on disconnect.
async fn ws_display_stream(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_stream_socket(socket, state))
}

async fn handle_stream_socket(mut socket: WebSocket, state: AppState) {
    tracing::info!("WebSocket stream client connected");

    {
        let mut s = state.status.lock().unwrap();
        s.state = DisplayState::Streaming;
        s.current_media = Some("websocket".to_string());
        s.frame = None;
        s.total_frames = None;
    }

    let mut frame_count: u64 = 0;

    while let Some(msg) = socket.recv().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("WebSocket receive error: {}", e);
                break;
            }
        };

        match msg {
            Message::Binary(data) => {
                let expected = state.panel.frame_byte_count();
                if data.len() != expected {
                    tracing::warn!(
                        "WebSocket frame: expected {} bytes, got {}",
                        expected,
                        data.len()
                    );
                    continue;
                }

                if state
                    .command_tx
                    .send(RenderCommand::ShowFrame(data.to_vec()))
                    .is_err()
                {
                    tracing::error!("Render thread gone, closing WebSocket");
                    break;
                }

                frame_count += 1;
            }
            Message::Close(_) => break,
            _ => {} // Ignore text, ping/pong handled by axum
        }
    }

    tracing::info!(
        "WebSocket stream client disconnected ({} frames received)",
        frame_count
    );
    state.status.lock().unwrap().set_idle();
}

// ── Path validation ──────────────────────────────────────────────────

/// Validate that a requested path is within the media directory.
///
/// This prevents directory traversal attacks (e.g., `../../etc/passwd`).
/// We canonicalize both paths and check that the requested path starts
/// with the media directory prefix.
fn validate_media_path(
    media_dir: &PathBuf,
    relative_path: &str,
) -> Result<PathBuf, (StatusCode, String)> {
    let full_path = media_dir.join(relative_path);

    // Canonicalize to resolve any `..` components
    let canonical = full_path.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            format!("Path not found: {relative_path}"),
        )
    })?;

    let canonical_media = media_dir.canonicalize().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Media directory not found".to_string(),
        )
    })?;

    if !canonical.starts_with(&canonical_media) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Path is outside the media directory".to_string(),
        ));
    }

    Ok(canonical)
}
