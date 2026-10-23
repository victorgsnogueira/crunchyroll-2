//! Video surface: a borderless WebView2 child of the app window that runs a local
//! Shaka Player page. DRM is handled by the WebView's own licensed CDM; this crate
//! only forwards bytes (manifest, license challenge/response, subtitle files).

pub mod sso;

use anyhow::{Result, anyhow};
use raw_window_handle::HasWindowHandle;
use serde::Deserialize;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::http::{Request, Response, StatusCode};
use wry::{Rect, RequestAsyncResponder, WebContext, WebView, WebViewBuilder};

#[cfg(windows)]
use wry::WebViewBuilderExtWindows;

const SCHEME: &str = "crplayer";

/// A request the page makes that the app must answer.
#[derive(Debug)]
pub enum ProxyRequest {
    Manifest,
    License { system: String, challenge: Vec<u8> },
    Text { url: String },
    /// Video/audio segment from the CDN. The CDN rejects requests coming from a
    /// web page origin (hotlink protection), so the app fetches it natively.
    Segment { url: String, range: Option<String> },
}

/// Answer for a `ProxyRequest`.
#[derive(Debug)]
pub struct ProxyResponse {
    pub status: u16,
    pub mime: String,
    pub content_range: Option<String>,
    pub body: Vec<u8>,
}

impl ProxyResponse {
    pub fn ok(mime: impl Into<String>, body: Vec<u8>) -> Self {
        Self { status: 200, mime: mime.into(), content_range: None, body }
    }
}

pub type ProxyResult = std::result::Result<ProxyResponse, String>;
pub type Reply = Box<dyn FnOnce(ProxyResult) + Send>;
pub type ProxyHandler = Arc<dyn Fn(ProxyRequest, Reply) + Send + Sync>;

/// Messages from the page.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PlayerEvent {
    Ready,
    Keysystems { found: serde_json::Map<String, serde_json::Value> },
    Loaded {
        #[serde(rename = "keySystem")]
        key_system: String,
        #[serde(rename = "maxHeight")]
        max_height: u32,
    },
    Stats {
        height: u32,
        width: u32,
        #[serde(rename = "trackHeight")]
        track_height: u32,
        bandwidth: u64,
        #[serde(rename = "keySystem")]
        key_system: String,
        dropped: u64,
    },
    Time { pos: f64, dur: f64, paused: bool },
    Ended,
    Next,
    Back { pos: f64 },
    Escape { pos: f64 },
    Fullscreen,
    Audio { media_id: String, position: f64 },
    Error { message: String, code: i64 },
}

pub struct Player {
    webview: WebView,
    _context: WebContext,
}

pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl From<&Bounds> for Rect {
    fn from(b: &Bounds) -> Rect {
        Rect {
            position: PhysicalPosition::new(b.x, b.y).into(),
            size: PhysicalSize::new(b.width.max(1), b.height.max(1)).into(),
        }
    }
}

impl Player {
    /// Creates the (hidden) player inside `parent`.
    pub fn new<W: HasWindowHandle>(
        parent: &W,
        bounds: Bounds,
        data_dir: PathBuf,
        proxy: ProxyHandler,
        on_event: impl Fn(PlayerEvent) + 'static,
    ) -> Result<Self> {
        let mut context = WebContext::new(Some(data_dir));
        let builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_bounds((&bounds).into())
            .with_visible(false)
            .with_background_color((0, 0, 0, 255))
            .with_autoplay(true)
            .with_devtools(cfg!(debug_assertions))
            .with_asynchronous_custom_protocol(SCHEME.into(), move |_id, req, responder| {
                route(req, responder, &proxy);
            })
            .with_ipc_handler(move |req: Request<String>| match serde_json::from_str::<PlayerEvent>(req.body()) {
                Ok(ev) => on_event(ev),
                Err(e) => tracing::debug!("mensagem do player ignorada: {e}: {}", req.body()),
            })
            .with_url(format!("{}://localhost/", SCHEME));

        #[cfg(windows)]
        let builder = {
            let mut args = String::from(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required",
            );
            if let Ok(extra) = std::env::var("CR_WEBVIEW_ARGS") {
                args.push(' ');
                args.push_str(&extra);
            }
            builder
                .with_https_scheme(true)
                .with_browser_accelerator_keys(false)
                .with_default_context_menus(cfg!(debug_assertions))
                .with_additional_browser_args(args)
        };

        let webview = builder.build_as_child(parent).map_err(|e| anyhow!("WebView2 indisponível: {e}"))?;
        Ok(Self { webview, _context: context })
    }

    pub fn set_bounds(&self, b: &Bounds) {
        let _ = self.webview.set_bounds(b.into());
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.webview.set_visible(visible);
        if visible {
            let _ = self.webview.focus();
        } else {
            let _ = self.webview.focus_parent();
        }
    }

    /// Starts a video. `info` is the serialized `PlaybackInfo`.
    pub fn load(&self, info: &serde_json::Value, debug: bool, drm_order: &[&str]) {
        let opts = serde_json::json!({ "debug": debug, "drmOrder": drm_order });
        self.eval(&format!("window.crPlayer && window.crPlayer.load({info}, {opts});"));
    }

    pub fn stop(&self) {
        self.eval("window.crPlayer && window.crPlayer.stop();");
    }

    pub fn eval(&self, js: &str) {
        if let Err(e) = self.webview.evaluate_script(js) {
            tracing::warn!("script do player falhou: {e}");
        }
    }

    pub fn open_devtools(&self) {
        #[cfg(debug_assertions)]
        self.webview.open_devtools();
    }
}

// ------------------------------------------------------------------- routing

struct Asset {
    mime: &'static str,
    body: &'static [u8],
}

fn asset(path: &str) -> Option<Asset> {
    macro_rules! a {
        ($mime:expr, $file:expr) => {
            Some(Asset { mime: $mime, body: include_bytes!($file) })
        };
    }
    match path {
        "/" | "/index.html" => a!("text/html; charset=utf-8", "../assets/player.html"),
        "/player.css" => a!("text/css; charset=utf-8", "../assets/player.css"),
        "/player.js" => a!("text/javascript; charset=utf-8", "../assets/player.js"),
        "/vendor/shaka-player.compiled.js" => a!("text/javascript", "../assets/vendor/shaka-player.compiled.js"),
        "/vendor/jassub.umd.js" => a!("text/javascript", "../assets/vendor/jassub.umd.js"),
        "/vendor/jassub-worker.js" => a!("text/javascript", "../assets/vendor/jassub-worker.js"),
        "/vendor/jassub-worker.wasm" => a!("application/wasm", "../assets/vendor/jassub-worker.wasm"),
        "/vendor/jassub-worker-modern.wasm" => a!("application/wasm", "../assets/vendor/jassub-worker-modern.wasm"),
        "/vendor/default.woff2" => a!("font/woff2", "../assets/vendor/default.woff2"),
        "/fonts/Lato-Regular.ttf" => a!("font/ttf", "../../../assets/fonts/Lato-Regular.ttf"),
        "/fonts/Lato-Bold.ttf" => a!("font/ttf", "../../../assets/fonts/Lato-Bold.ttf"),
        "/fonts/Lato-Black.ttf" => a!("font/ttf", "../../../assets/fonts/Lato-Black.ttf"),
        _ => None,
    }
}

fn respond(responder: RequestAsyncResponder, status: StatusCode, mime: &str, body: Cow<'static, [u8]>) {
    respond_full(responder, status, mime, None, body)
}

fn respond_full(
    responder: RequestAsyncResponder,
    status: StatusCode,
    mime: &str,
    content_range: Option<&str>,
    body: Cow<'static, [u8]>,
) {
    let mut b = Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", "no-store")
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Expose-Headers", "Content-Range, Content-Length");
    if let Some(cr) = content_range {
        b = b.header("Content-Range", cr);
    }
    let resp = b.body(body).unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..])));
    responder.respond(resp);
}

/// Value of `key` in a query string, percent-decoded.
pub fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn route(req: Request<Vec<u8>>, responder: RequestAsyncResponder, proxy: &ProxyHandler) {
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();

    if let Some(a) = asset(&path) {
        return respond(responder, StatusCode::OK, a.mime, Cow::Borrowed(a.body));
    }

    let request = match path.as_str() {
        "/proxy/manifest" => ProxyRequest::Manifest,
        "/proxy/license" => ProxyRequest::License {
            system: query_param(&query, "system").unwrap_or_else(|| "widevine".into()),
            challenge: req.into_body(),
        },
        "/proxy/text" => match query_param(&query, "u") {
            Some(url) if url.starts_with("https://") => ProxyRequest::Text { url },
            _ => return respond(responder, StatusCode::BAD_REQUEST, "text/plain", Cow::Borrowed(b"bad url")),
        },
        "/proxy/seg" => match query_param(&query, "u") {
            Some(url) if url.starts_with("https://") => {
                let range = req.headers().get("range").and_then(|v| v.to_str().ok()).map(String::from);
                ProxyRequest::Segment { url, range }
            }
            _ => return respond(responder, StatusCode::BAD_REQUEST, "text/plain", Cow::Borrowed(b"bad url")),
        },
        _ => return respond(responder, StatusCode::NOT_FOUND, "text/plain", Cow::Borrowed(b"not found")),
    };

    let reply: Reply = Box::new(move |result| match result {
        Ok(r) => {
            let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::OK);
            respond_full(responder, status, &r.mime, r.content_range.as_deref(), Cow::Owned(r.body))
        }
        Err(msg) => {
            tracing::warn!("proxy do player: {msg}");
            respond(responder, StatusCode::BAD_GATEWAY, "text/plain; charset=utf-8", Cow::Owned(msg.into_bytes()))
        }
    });
    proxy(request, reply);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_decode() {
        assert_eq!(query_param("system=playready&x=1", "system").as_deref(), Some("playready"));
        assert_eq!(
            query_param("u=https%3A%2F%2Fcdn.x%2Fa.ass%3Ft%3D1", "u").as_deref(),
            Some("https://cdn.x/a.ass?t=1")
        );
        assert_eq!(query_param("a=1", "b"), None);
        assert_eq!(query_param("u=50%", "u").as_deref(), Some("50%"));
    }

    #[test]
    fn page_assets_exist() {
        for p in ["/", "/player.js", "/player.css", "/vendor/shaka-player.compiled.js", "/vendor/jassub-worker.wasm", "/fonts/Lato-Bold.ttf"] {
            let a = asset(p).unwrap_or_else(|| panic!("faltando {p}"));
            assert!(!a.body.is_empty(), "{p} vazio");
        }
        assert!(asset("/nope").is_none());
    }

    #[test]
    fn events_parse() {
        let e: PlayerEvent = serde_json::from_str(r#"{"type":"loaded","keySystem":"com.widevine.alpha","maxHeight":1080}"#).unwrap();
        assert!(matches!(e, PlayerEvent::Loaded { max_height: 1080, .. }));
        let e: PlayerEvent = serde_json::from_str(r#"{"type":"time","pos":1.5,"dur":1400,"paused":false}"#).unwrap();
        assert!(matches!(e, PlayerEvent::Time { paused: false, .. }));
    }
}
