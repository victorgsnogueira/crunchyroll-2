//! Owns the WebView2 player: places it over the window, feeds it streams and
//! reacts to its events (progress, next episode, audio switch, exit).

use crate::app::App;
use cr_core::session::{self, Session};
use cr_core::{DrmSystem, Playback};
use cr_player::{Bounds, Player, PlayerEvent, ProxyHandler, ProxyRequest, ProxyResponse};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct Active {
    session: Arc<Session>,
    pb: Arc<Playback>,
}

pub struct PlayerCtl {
    rt: tokio::runtime::Handle,
    view: RefCell<Option<Player>>,
    ready: Cell<bool>,
    pending: RefCell<Option<serde_json::Value>>,
    active: Arc<Mutex<Option<Active>>>,
    last_pos: Cell<f64>,
    last_saved: Cell<f64>,
    seq: Cell<u64>,
    bounds_timer: slint::Timer,
    last_bounds: Cell<(u32, u32, bool)>,
    best_height: Cell<u32>,
}

impl PlayerCtl {
    pub fn new(rt: tokio::runtime::Handle) -> Self {
        Self {
            rt,
            view: RefCell::new(None),
            ready: Cell::new(false),
            pending: RefCell::new(None),
            active: Arc::new(Mutex::new(None)),
            last_pos: Cell::new(0.0),
            last_saved: Cell::new(0.0),
            seq: Cell::new(0),
            bounds_timer: slint::Timer::default(),
            last_bounds: Cell::new((0, 0, false)),
            best_height: Cell::new(0),
        }
    }

    pub fn last_position(&self) -> f64 {
        self.last_pos.get()
    }

    pub fn take_active(&self) -> Option<Arc<Playback>> {
        self.active.lock().unwrap().take().map(|a| a.pb)
    }

    fn current(&self) -> Option<Active> {
        self.active.lock().unwrap().clone()
    }

    /// Starts playing `id` (episode/movie, or a series when `series`).
    pub fn open(&self, app: &Rc<App>, s: Arc<Session>, id: String, series: bool) {
        self.open_at(app, s, id, series, None);
    }

    fn open_at(&self, app: &Rc<App>, s: Arc<Session>, id: String, series: bool, resume: Option<f64>) {
        let seq = self.seq.get() + 1;
        self.seq.set(seq);
        self.release_current(app);
        self.best_height.set(0);
        self.last_pos.set(0.0);
        self.last_saved.set(0.0);

        let ui = app.ui();
        ui.set_screen("player".into());
        ui.set_player_loading(true);
        if let Err(e) = self.ensure_view(app) {
            app.toast(format!("Player indisponível: {e:#}"));
            return self.close(app);
        }
        self.update_bounds(app);
        if let Some(v) = self.view.borrow().as_ref() {
            v.set_visible(true);
        }
        self.start_bounds_timer(app);

        app.run(
            async move {
                let target = if series {
                    s.up_next(&id).await.ok().flatten().map(|(ep, _)| ep).unwrap_or(id)
                } else {
                    id
                };
                let pb = s.play(&target).await;
                (s, pb)
            },
            move |app, (s, pb)| {
                let ctl = &app.player;
                let pb = match pb {
                    Ok(pb) => Arc::new(pb),
                    Err(e) => {
                        app.toast(format!("{e:#}"));
                        return ctl.close(app);
                    }
                };
                if ctl.seq.get() != seq {
                    // user already left or opened something else
                    let rt = ctl.rt.clone();
                    rt.spawn(async move { pb.close().await });
                    return;
                }
                let mut info = pb.info.clone();
                if let Some(r) = resume {
                    info.resume_at = r.max(0.0) as u32;
                }
                *ctl.active.lock().unwrap() = Some(Active { session: s, pb });
                let json = serde_json::to_value(&info).unwrap_or_default();
                ctl.load_json(json);
            },
        );
    }

    fn load_json(&self, json: serde_json::Value) {
        if self.ready.get() {
            if let Some(v) = self.view.borrow().as_ref() {
                v.load(&json, std::env::var("CR_PLAYER_DEBUG").is_ok(), &drm_order());
            }
        } else {
            *self.pending.borrow_mut() = Some(json);
        }
    }

    fn ensure_view(&self, app: &Rc<App>) -> anyhow::Result<()> {
        if self.view.borrow().is_some() {
            return Ok(());
        }
        let ui = app.ui();
        let handle = ui.window().window_handle();
        let weak: Weak<App> = Rc::downgrade(app);
        let player = Player::new(
            &handle,
            Bounds { x: 0, y: 0, width: 1, height: 1 },
            session::data_dir().join("webview"),
            self.proxy(),
            move |ev| {
                if let Some(app) = weak.upgrade() {
                    app.player.on_event(&app, ev);
                }
            },
        )?;
        *self.view.borrow_mut() = Some(player);
        Ok(())
    }

    fn proxy(&self) -> ProxyHandler {
        let active = self.active.clone();
        let rt = self.rt.clone();
        // one pooled HTTP client for segments (keeps CDN connections alive)
        let seg_http = reqwest::Client::builder()
            .user_agent("Crunchyroll/3.126.0 Android/14 okhttp/4.12.0")
            .pool_max_idle_per_host(8)
            .build()
            .unwrap_or_default();
        Arc::new(move |req: ProxyRequest, reply| {
            let a = active.lock().unwrap().clone();
            let seg_http = seg_http.clone();
            rt.spawn(async move {
                let Some(a) = a else {
                    return reply(Err("nenhum vídeo ativo".into()));
                };
                let kind = match &req {
                    ProxyRequest::Manifest => "manifesto",
                    ProxyRequest::License { .. } => "licença",
                    ProxyRequest::Text { .. } => "legenda",
                    ProxyRequest::Segment { .. } => "segmento",
                };
                let result = match req {
                    ProxyRequest::Manifest => a
                        .session
                        .manifest(&a.pb)
                        .await
                        .map(|m| ProxyResponse::ok("application/dash+xml", m.into_bytes())),
                    ProxyRequest::License { system, challenge } => {
                        let sys = if system == "playready" { DrmSystem::PlayReady } else { DrmSystem::Widevine };
                        a.session.license(&a.pb, sys, challenge).await.map(|b| ProxyResponse::ok("application/octet-stream", b))
                    }
                    ProxyRequest::Text { url } => fetch_text(&a.session, &url).await,
                    ProxyRequest::Segment { url, range } => fetch_segment(&seg_http, &url, range.as_deref()).await,
                };
                match &result {
                    Ok(r) if kind == "segmento" => tracing::debug!("proxy segmento: {} {} bytes", r.status, r.body.len()),
                    Ok(r) => tracing::info!("proxy {kind}: {} {} {} bytes", r.status, r.mime, r.body.len()),
                    Err(e) => tracing::warn!("proxy {kind}: {e:#}"),
                }
                reply(result.map_err(|e| format!("{e:#}")));
            });
        })
    }

    pub fn update_bounds(&self, app: &Rc<App>) {
        let ui = app.ui();
        let win = ui.window();
        let size = win.size();
        let full = ui.get_fullscreen();
        let top = if full { 0 } else { (36.0 * win.scale_factor()).round() as u32 };
        self.last_bounds.set((size.width, size.height, full));
        if let Some(v) = self.view.borrow().as_ref() {
            v.set_bounds(&Bounds { x: 0, y: top as i32, width: size.width, height: size.height.saturating_sub(top) });
        }
    }

    fn start_bounds_timer(&self, app: &Rc<App>) {
        let weak = Rc::downgrade(app);
        self.bounds_timer.start(slint::TimerMode::Repeated, Duration::from_millis(120), move || {
            let Some(app) = weak.upgrade() else { return };
            let ui = app.ui();
            let size = ui.window().size();
            let now = (size.width, size.height, ui.get_fullscreen());
            if now != app.player.last_bounds.get() {
                app.player.update_bounds(&app);
            }
        });
    }

    /// Saves progress and frees the current stream (in the background).
    fn release_current(&self, _app: &Rc<App>) {
        let Some(a) = self.active.lock().unwrap().take() else { return };
        let pos = self.last_pos.get();
        self.rt.spawn(async move {
            if pos > 1.0 {
                a.pb.save_progress(pos as u32).await;
            }
            a.pb.close().await;
        });
    }

    pub fn close(&self, app: &Rc<App>) {
        self.seq.set(self.seq.get() + 1);
        self.release_current(app);
        if let Some(v) = self.view.borrow().as_ref() {
            v.stop();
            v.set_visible(false);
        }
        self.bounds_timer.stop();
        let ui = app.ui();
        if ui.get_fullscreen() {
            app.set_fullscreen(false);
        }
        ui.set_player_loading(false);
        ui.set_screen("main".into());
        app.after_player_closed();
    }

    fn save_progress(&self, pos: f64) {
        let Some(a) = self.current() else { return };
        self.last_saved.set(pos);
        self.rt.spawn(async move { a.pb.save_progress(pos as u32).await });
    }

    fn on_event(&self, app: &Rc<App>, ev: PlayerEvent) {
        match ev {
            PlayerEvent::Ready => {
                self.ready.set(true);
                if let Some(json) = self.pending.borrow_mut().take() {
                    self.load_json(json);
                }
            }
            PlayerEvent::Keysystems { found } => {
                let names: Vec<String> = found.iter().map(|(k, v)| format!("{k}={v}")).collect();
                check_log(&format!("DRM disponível no WebView2: {}", if names.is_empty() { "nenhum".into() } else { names.join(", ") }));
            }
            PlayerEvent::Loaded { key_system, max_height } => {
                app.ui().set_player_loading(false);
                let id = self.current().map(|a| a.pb.info.episode_id.clone()).unwrap_or_default();
                check_log(&format!("carregado {id}: DRM {key_system}, maior faixa {max_height}p"));
            }
            PlayerEvent::Stats { height, width, track_height, key_system, bandwidth, dropped } => {
                if height > self.best_height.get() {
                    self.best_height.set(height);
                    check_log(&format!(
                        "decodificando {width}x{height} (faixa {track_height}p, {:.1} Mb/s, {dropped} quadros perdidos) via {key_system}{}",
                        bandwidth as f64 / 1e6,
                        if height >= 1080 { "  >> 1080p OK" } else { "" }
                    ));
                }
            }
            PlayerEvent::Time { pos, paused, .. } => {
                self.last_pos.set(pos);
                if paused || (pos - self.last_saved.get()).abs() >= 15.0 {
                    self.save_progress(pos);
                }
            }
            PlayerEvent::Ended => {
                let has_next = self.current().map(|a| a.pb.info.next.is_some()).unwrap_or(false);
                if !has_next {
                    self.close(app);
                }
            }
            PlayerEvent::Next => {
                let next = self.current().and_then(|a| a.pb.info.next.clone().map(|n| (a.session, n.id)));
                if let Some((s, id)) = next {
                    self.open(app, s, id, false);
                }
            }
            PlayerEvent::Back { .. } => self.close(app),
            PlayerEvent::Escape { .. } => {
                if app.ui().get_fullscreen() {
                    app.set_fullscreen(false);
                } else {
                    self.close(app);
                }
            }
            PlayerEvent::Fullscreen => {
                let on = !app.ui().get_fullscreen();
                app.set_fullscreen(on);
            }
            PlayerEvent::Audio { media_id, position } => {
                if let Some(a) = self.current() {
                    self.last_pos.set(position);
                    self.open_at(app, a.session, media_id, false, Some(position));
                }
            }
            PlayerEvent::Log { message } => tracing::info!("player: {message}"),
            PlayerEvent::Error { message, code } => {
                app.ui().set_player_loading(false);
                check_log(&format!("erro do player ({code}): {message}"));
            }
        }
    }
}

fn drm_order() -> Vec<&'static str> {
    match std::env::var("CR_DRM_ORDER").as_deref() {
        Ok("playready") => vec!["playready"],
        Ok("widevine") => vec!["widevine"],
        Ok("playready,widevine") => vec!["playready", "widevine"],
        _ => vec!["widevine", "playready"],
    }
}

async fn fetch_text(s: &Session, url: &str) -> anyhow::Result<ProxyResponse> {
    let body = s.cr.client().get(url).send().await?.error_for_status()?.bytes().await?;
    Ok(ProxyResponse::ok("text/plain; charset=utf-8", body.to_vec()))
}

/// Fetches a CDN segment natively (no web origin), honoring byte ranges.
async fn fetch_segment(http: &reqwest::Client, url: &str, range: Option<&str>) -> anyhow::Result<ProxyResponse> {
    let mut req = http.get(url);
    if let Some(r) = range {
        req = req.header("Range", r);
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let mime = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let content_range = resp.headers().get("content-range").and_then(|v| v.to_str().ok()).map(String::from);
    let body = resp.bytes().await?.to_vec();
    if status >= 400 {
        anyhow::bail!("CDN respondeu {status} para {url}");
    }
    Ok(ProxyResponse { status, mime, content_range, body })
}

/// Appends a line to the playback check log (used to confirm the 1080p test).
fn check_log(line: &str) {
    tracing::info!("{line}");
    let path = session::data_dir().join("playback-check.log");
    let _ = std::fs::create_dir_all(session::data_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {line}", chrono_now());
    }
}

fn chrono_now() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("[{}]", d.as_secs())
}
