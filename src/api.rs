//! HTTP + WebSocket API for the companion app, served over the tailnet.
//!
//! Only loopback and tailnet peers are served (see [`crate::access`]); LAN
//! callers get 403 before auth runs. Every `/api` route is behind a shared
//! bearer token (header `Authorization: Bearer <token>` or `?token=<token>`
//! for WebSocket / media URLs that can't set headers). Real-time updates are
//! pushed over `/api/events`; everything else is plain JSON, plus
//! range-enabled MP4 streaming for clips.
//!
//! `/internal/mediamtx-auth` is MediaMTX's HTTP auth hook (loopback only), so
//! live WebRTC viewing is gated by the same token as the API.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path as AxumPath, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tower::ServiceExt;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeFile;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use crate::access;
use crate::clips;
use crate::config::RecordingConfig;
use crate::events::ApiEvent;
use crate::live::LiveHub;
use crate::recorder::capture_snapshot;
use crate::state::SharedState;

/// Everything the handlers need, cheaply cloneable per request.
#[derive(Clone)]
pub struct AppState {
    pub state: Arc<SharedState>,
    pub live: Arc<LiveHub>,
    pub events_tx: broadcast::Sender<ApiEvent>,
    pub rec_cfg: RecordingConfig,
    pub token: Arc<String>,
}

/// Bind the API on `0.0.0.0:<port>` and serve loopback + tailnet peers only.
pub async fn run(app_state: AppState, port: u16) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    info!("api listening on http://0.0.0.0:{port} (localhost + tailnet only)");
    axum::serve(
        listener,
        router(app_state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn router(app_state: AppState) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/arm", post(arm))
        .route("/disarm", post(disarm))
        .route("/threshold", post(threshold))
        .route("/snapshot", post(snapshot))
        .route("/clips", get(list_clips))
        .route("/clips/{name}", get(get_clip))
        .route("/live/start", post(live_start))
        .route("/live/stop", post(live_stop))
        .route("/events", get(events_ws))
        .layer(middleware::from_fn_with_state(app_state.clone(), auth))
        .with_state(app_state.clone());

    let internal = Router::new()
        .route("/mediamtx-auth", post(mediamtx_auth))
        .layer(middleware::from_fn(loopback_only))
        .with_state(app_state);

    Router::new()
        .nest("/api", api)
        .nest("/internal", internal)
        .layer(middleware::from_fn(trusted_peers_only))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// Broadcast the live audio level at ~10 Hz (the cpal callback fires far faster;
/// the WS would be flooded if it forwarded every buffer).
pub async fn run_level_broadcaster(
    state: Arc<SharedState>,
    events_tx: broadcast::Sender<ApiEvent>,
) {
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tick.tick().await;
        let _ = events_tx.send(ApiEvent::Level {
            db: state.current_db(),
        });
    }
}

// --- auth ------------------------------------------------------------------

/// Reject anything that isn't loopback or a tailnet peer, so the API isn't
/// reachable (or token-guessable) from the LAN even though it binds 0.0.0.0.
async fn trusted_peers_only(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if access::is_trusted_peer(peer.ip()) {
        next.run(req).await
    } else {
        warn!(%peer, path = req.uri().path(), "api: rejecting non-tailnet client");
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}

async fn loopback_only(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if access::is_loopback(peer.ip()) {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}

async fn auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match request_token(&req) {
        Some(t) if access::token_eq(&t, &st.token) => next.run(req).await,
        _ => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
    }
}

fn request_token(req: &Request) -> Option<String> {
    if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") {
                return Some(t.trim().to_string());
            }
        }
    }
    req.uri().query().and_then(|q| query_value(q, "token"))
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

// --- control ---------------------------------------------------------------

#[derive(Serialize)]
struct StatusResponse {
    armed: bool,
    recording: bool,
    current_db: f32,
    trigger_db: f32,
    last_trigger: Option<DateTime<Local>>,
    last_clip: Option<String>,
}

fn status_response(state: &SharedState) -> StatusResponse {
    StatusResponse {
        armed: state.armed(),
        recording: state.recording(),
        current_db: state.current_db(),
        trigger_db: state.trigger_db(),
        last_trigger: state.last_trigger(),
        last_clip: state
            .last_clip()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
    }
}

/// Push a `State` event so connected clients update immediately.
fn broadcast_state(st: &AppState) {
    let _ = st.events_tx.send(ApiEvent::State {
        armed: st.state.armed(),
        recording: st.state.recording(),
    });
}

async fn status(State(st): State<AppState>) -> Json<StatusResponse> {
    Json(status_response(&st.state))
}

async fn arm(State(st): State<AppState>) -> Json<StatusResponse> {
    st.state.set_armed(true);
    broadcast_state(&st);
    Json(status_response(&st.state))
}

async fn disarm(State(st): State<AppState>) -> Json<StatusResponse> {
    st.state.set_armed(false);
    broadcast_state(&st);
    Json(status_response(&st.state))
}

#[derive(Deserialize)]
struct ThresholdReq {
    db: f32,
}

async fn threshold(
    State(st): State<AppState>,
    Json(body): Json<ThresholdReq>,
) -> Result<Json<StatusResponse>, (StatusCode, &'static str)> {
    if !SharedState::valid_trigger_db(body.db) {
        return Err((
            StatusCode::BAD_REQUEST,
            "db must be between -120 and 0 dBFS",
        ));
    }
    st.state.set_trigger_db(body.db);
    broadcast_state(&st);
    Ok(Json(status_response(&st.state)))
}

#[derive(Serialize)]
struct UrlResponse {
    url: String,
}

async fn snapshot(State(st): State<AppState>) -> Result<Json<UrlResponse>, ApiError> {
    let path = capture_snapshot(&st.live, &st.rec_cfg).await?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow::anyhow!("snapshot path had no file name"))?;
    Ok(Json(UrlResponse {
        url: format!("/api/clips/{name}"),
    }))
}

// --- live ------------------------------------------------------------------

#[derive(Serialize)]
struct WhepResponse {
    whep_url: String,
}

async fn live_start(State(st): State<AppState>) -> Result<Json<WhepResponse>, ApiError> {
    st.live.acquire_live().await?;
    Ok(Json(WhepResponse {
        whep_url: st.live.whep_url().to_string(),
    }))
}

async fn live_stop(State(st): State<AppState>) -> StatusCode {
    st.live.release_live().await;
    StatusCode::NO_CONTENT
}

// --- clips -----------------------------------------------------------------

#[derive(Serialize)]
struct ClipInfoResponse {
    name: String,
    started_at: DateTime<Local>,
    duration_s: f32,
    size_bytes: u64,
    thumb_url: Option<String>,
}

impl From<clips::ClipInfo> for ClipInfoResponse {
    fn from(c: clips::ClipInfo) -> Self {
        let thumb_url = c.thumb_path.and_then(|p| {
            p.file_name()
                .map(|n| format!("/api/clips/{}", n.to_string_lossy()))
        });
        Self {
            name: c.name,
            started_at: c.started_at,
            duration_s: c.duration_s,
            size_bytes: c.size_bytes,
            thumb_url,
        }
    }
}

async fn list_clips(State(st): State<AppState>) -> Result<Json<Vec<ClipInfoResponse>>, ApiError> {
    let clips = clips::list(&st.rec_cfg.output_dir).await?;
    Ok(Json(
        clips.into_iter().map(ClipInfoResponse::from).collect(),
    ))
}

async fn get_clip(
    State(st): State<AppState>,
    AxumPath(name): AxumPath<String>,
    req: Request,
) -> Response {
    if !is_safe_name(&name) {
        return (StatusCode::BAD_REQUEST, "bad clip name").into_response();
    }
    let path = st.rec_cfg.output_dir.join(&name);
    // ServeFile handles Range requests (206 / partial content) and 404s.
    match ServeFile::new(&path).oneshot(req).await {
        Ok(res) => res.map(axum::body::Body::new).into_response(),
        Err(e) => {
            error!("serving clip {name}: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

// --- events websocket ------------------------------------------------------

async fn events_ws(State(st): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let rx = st.events_tx.subscribe();
    ws.on_upgrade(move |socket| forward_events(socket, rx))
}

async fn forward_events(mut socket: WebSocket, mut rx: broadcast::Receiver<ApiEvent>) {
    loop {
        match rx.recv().await {
            Ok(event) => {
                let Ok(text) = serde_json::to_string(&event) else {
                    continue;
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break; // client gone
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

// --- MediaMTX auth hook ----------------------------------------------------

/// The body MediaMTX POSTs to `authHTTPAddress` for every publish/read
/// (`authMethod: http` in `mediamtx.yml`). Fields we don't use are ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MediaMtxAuthReq {
    user: String,
    password: String,
    /// Bearer token from the client's `Authorization` header, if any.
    token: String,
    /// Client IP as MediaMTX saw it.
    ip: String,
    /// `publish`, `read`, `playback`, `api`, `metrics` or `pprof`.
    action: String,
    path: String,
    protocol: String,
    /// Raw query string of the client's request (e.g. `token=…`).
    query: String,
}

/// MediaMTX's answer is the status code alone: 2xx allows, anything else
/// denies. Since MediaMTX can't reach this hook while Dogitor is down, the
/// camera stream fails closed.
async fn mediamtx_auth(State(st): State<AppState>, Json(req): Json<MediaMtxAuthReq>) -> StatusCode {
    if media_allowed(&req, &st.token) {
        StatusCode::NO_CONTENT
    } else {
        warn!(
            ip = req.ip,
            action = req.action,
            path = req.path,
            protocol = req.protocol,
            user = req.user,
            "mediamtx: denied"
        );
        StatusCode::UNAUTHORIZED
    }
}

/// - localhost (Dogitor's own FFmpeg publisher and clip/snapshot readers):
///   may publish and read;
/// - tailnet peers: may read (watch live) with the API token, sent as a
///   bearer header, as the password, or as `?token=` on the WHEP URL;
/// - everyone else, and every other action: denied.
fn media_allowed(req: &MediaMtxAuthReq, token: &str) -> bool {
    let Ok(ip) = req.ip.parse::<IpAddr>() else {
        return false;
    };
    if access::is_loopback(ip) {
        return matches!(req.action.as_str(), "publish" | "read");
    }
    if req.action != "read" || !access::is_tailnet(ip) {
        return false;
    }
    let from_query = query_value(&req.query, "token").unwrap_or_default();
    let presented = [
        req.token.as_str(),
        req.password.as_str(),
        from_query.as_str(),
    ];
    presented.iter().any(|t| access::token_eq(t, token))
}

// --- error -----------------------------------------------------------------

/// Wraps any error into a 500 response while logging the full chain.
struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error!("api error: {:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_value_finds_key() {
        assert_eq!(query_value("token=abc", "token").as_deref(), Some("abc"));
        assert_eq!(
            query_value("a=1&token=xyz&b=2", "token").as_deref(),
            Some("xyz")
        );
        assert_eq!(query_value("a=1&b=2", "token"), None);
        assert_eq!(query_value("", "token"), None);
    }

    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;

    const TOKEN: &str = "test-token";

    fn test_state() -> AppState {
        let (events_tx, _) = broadcast::channel(4);
        AppState {
            state: Arc::new(SharedState::new(true, 0, -30.0)),
            live: LiveHub::new(
                crate::audio::AudioFormat {
                    sample_rate: 48_000,
                    channels: 1,
                },
                RecordingConfig::default(),
                Default::default(),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ),
            events_tx,
            rec_cfg: RecordingConfig::default(),
            token: Arc::new(TOKEN.to_string()),
        }
    }

    /// Send `req` through the real router as if it came from `peer`.
    async fn send_from(peer: &str, req: axum::http::Request<Body>) -> StatusCode {
        let addr: SocketAddr = format!("{peer}:50000").parse().unwrap();
        router(test_state())
            .layer(MockConnectInfo(addr))
            .oneshot(req)
            .await
            .unwrap()
            .status()
    }

    fn get_status(token: Option<&str>) -> axum::http::Request<Body> {
        let mut b = axum::http::Request::get("/api/status");
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    fn hook_req(ip: &str, action: &str) -> axum::http::Request<Body> {
        axum::http::Request::post("/internal/mediamtx-auth")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(
                r#"{{"ip":"{ip}","action":"{action}","path":"cam","protocol":"rtsp"}}"#
            )))
            .unwrap()
    }

    #[tokio::test]
    async fn api_rejects_lan_even_with_token() {
        assert_eq!(
            send_from("192.168.1.20", get_status(Some(TOKEN))).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn api_serves_tailnet_and_localhost_with_token() {
        assert_eq!(
            send_from("100.90.1.2", get_status(Some(TOKEN))).await,
            StatusCode::OK
        );
        assert_eq!(
            send_from("127.0.0.1", get_status(Some(TOKEN))).await,
            StatusCode::OK
        );
        assert_eq!(
            send_from("100.90.1.2", get_status(Some("wrong"))).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            send_from("100.90.1.2", get_status(None)).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn threshold_rejects_out_of_range() {
        let req = |db: &str| {
            axum::http::Request::post("/api/threshold")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"db":{db}}}"#)))
                .unwrap()
        };
        assert_eq!(send_from("127.0.0.1", req("-25")).await, StatusCode::OK);
        assert_eq!(
            send_from("127.0.0.1", req("12")).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send_from("127.0.0.1", req("-1e9")).await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn mediamtx_hook_is_loopback_only() {
        assert_eq!(
            send_from("127.0.0.1", hook_req("127.0.0.1", "publish")).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            send_from("127.0.0.1", hook_req("192.168.1.20", "read")).await,
            StatusCode::UNAUTHORIZED
        );
        // A tailnet device can't call the hook itself to test tokens.
        assert_eq!(
            send_from("100.90.1.2", hook_req("127.0.0.1", "publish")).await,
            StatusCode::FORBIDDEN
        );
    }

    fn media_req(ip: &str, action: &str) -> MediaMtxAuthReq {
        MediaMtxAuthReq {
            ip: ip.into(),
            action: action.into(),
            path: "cam".into(),
            ..Default::default()
        }
    }

    #[test]
    fn mediamtx_localhost_may_publish_and_read() {
        assert!(media_allowed(&media_req("127.0.0.1", "publish"), "tok"));
        assert!(media_allowed(&media_req("::1", "read"), "tok"));
        assert!(!media_allowed(&media_req("127.0.0.1", "playback"), "tok"));
        assert!(!media_allowed(&media_req("127.0.0.1", "api"), "tok"));
    }

    #[test]
    fn mediamtx_tailnet_reads_need_token() {
        let no_token = media_req("100.90.1.2", "read");
        assert!(!media_allowed(&no_token, "tok"));

        let bearer = MediaMtxAuthReq {
            token: "tok".into(),
            ..media_req("100.90.1.2", "read")
        };
        assert!(media_allowed(&bearer, "tok"));

        let query = MediaMtxAuthReq {
            query: "a=1&token=tok".into(),
            ..media_req("100.90.1.2", "read")
        };
        assert!(media_allowed(&query, "tok"));

        let password = MediaMtxAuthReq {
            password: "tok".into(),
            ..media_req("100.90.1.2", "read")
        };
        assert!(media_allowed(&password, "tok"));

        let wrong = MediaMtxAuthReq {
            token: "nope".into(),
            ..media_req("100.90.1.2", "read")
        };
        assert!(!media_allowed(&wrong, "tok"));
    }

    #[test]
    fn mediamtx_denies_lan_and_remote_publish() {
        // Right token, wrong network.
        let lan = MediaMtxAuthReq {
            token: "tok".into(),
            ..media_req("192.168.1.20", "read")
        };
        assert!(!media_allowed(&lan, "tok"));
        // Nobody but localhost may publish, token or not.
        let publish = MediaMtxAuthReq {
            token: "tok".into(),
            ..media_req("100.90.1.2", "publish")
        };
        assert!(!media_allowed(&publish, "tok"));
        assert!(!media_allowed(&media_req("not-an-ip", "read"), "tok"));
    }

    #[test]
    fn safe_name_rejects_traversal() {
        assert!(is_safe_name("clip-20260621-153012.mp4"));
        assert!(is_safe_name("snap-20260621-153012.jpg"));
        assert!(!is_safe_name(""));
        assert!(!is_safe_name(".."));
        assert!(!is_safe_name("../config.toml"));
        assert!(!is_safe_name("sub/dir.mp4"));
        assert!(!is_safe_name("a\\b.mp4"));
    }
}
