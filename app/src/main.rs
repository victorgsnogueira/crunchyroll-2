#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod app;
mod convert;
mod images;
mod player_ctl;

use app::App;
use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use std::rc::Rc;

fn main() -> anyhow::Result<()> {
    let log_dir = cr_core::session::data_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("app.log"))?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,crunchyroll_rs=warn,wry=warn".into()),
        )
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false)
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build()?;
    let images = images::Images::new(
        cr_core::images::ImageCache::new(cr_core::images::default_cache_dir())?,
        runtime.handle().clone(),
    );

    let ui = AppWindow::new()?;
    let app = App::new(&ui, runtime.handle().clone(), images);
    wire(&ui, &app);
    app.start();

    ui.run()?;
    app.shutdown();
    drop(runtime);
    Ok(())
}

fn wire(ui: &AppWindow, app: &Rc<App>) {
    macro_rules! on {
        ($setter:ident, |$app:ident $(, $arg:ident)*| $body:expr) => {{
            let weak = Rc::downgrade(app);
            ui.$setter(move |$($arg),*| {
                if let Some($app) = weak.upgrade() {
                    $body
                }
            });
        }};
    }

    on!(on_login, |a, e, p| a.login(e.to_string(), p.to_string()));
    on!(on_start_code, |a| a.start_code());
    on!(on_cancel_code, |a| a.cancel_code());
    on!(on_open_activate, |a| a.open_activate());
    on!(on_pick_profile, |a, id| a.pick_profile(id.to_string()));
    on!(on_logout, |a| a.logout());
    on!(on_navigate, |a, p| a.navigate(p.to_string()));
    on!(on_open_media, |a, id| a.open_detail(id.to_string()));
    on!(on_card_clicked, |a, c| a.card_clicked(c));
    on!(on_play, |a, id| {
        // hero buttons pass a series id, everything else an episode/movie id
        let is_series = a.ui().get_screen() == "main" && !a.ui().get_detail_open();
        a.play(id.to_string(), is_series)
    });
    on!(on_close_detail, |a| a.ui().set_detail_open(false));
    on!(on_toggle_watchlist, |a| a.toggle_watchlist());
    on!(on_select_season, |a, id| a.load_episodes(id.to_string()));
    on!(on_rate, |a, s| a.rate(s));
    on!(on_search, |a, q| a.search(q.to_string()));
    on!(on_browse, |a, s, g| a.browse(s.to_string(), g.to_string()));
    on!(on_open_episode, |a, _series, ep| a.play(ep.to_string(), false));
    on!(on_select_list, |a, id| a.select_list(id.to_string()));
    on!(on_create_list, |a, n| a.create_list(n.to_string()));
    on!(on_delete_list, |a, id| a.delete_list(id.to_string()));
    on!(on_clear_history, |a| a.clear_history());
    on!(on_key_escape, |a| a.escape());

    on!(on_window_drag, |a| {
        a.ui().window().with_winit_window(|w| {
            let _ = w.drag_window();
        });
    });
    on!(on_window_resize, |a, dir| {
        use slint::winit_030::winit::window::ResizeDirection as D;
        let d = match dir.as_str() {
            "n" => D::North,
            "s" => D::South,
            "e" => D::East,
            "w" => D::West,
            "ne" => D::NorthEast,
            "nw" => D::NorthWest,
            "se" => D::SouthEast,
            _ => D::SouthWest,
        };
        a.ui().window().with_winit_window(|w| {
            let _ = w.drag_resize_window(d);
        });
    });
    on!(on_window_minimize, |a| a.ui().window().set_minimized(true));
    on!(on_window_maximize, |a| {
        let ui = a.ui();
        let w = ui.window();
        w.set_maximized(!w.is_maximized());
    });
    on!(on_window_close, |a| {
        let _ = a.ui().hide();
    });
}
