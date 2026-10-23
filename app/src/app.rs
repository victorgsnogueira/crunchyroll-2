//! App controller: wires UI callbacks to the Crunchyroll session, keeps models
//! up to date and drives the player.

use crate::convert;
use crate::images::Images;
use crate::player_ctl::PlayerCtl;
use crate::{AppWindow, CardData, CalendarDayData, EpisodeData, GenreData, HeroData, ListData, ProfileData, RowData, SeasonData};
use cr_core::models::{HomeData, MediaCard};
use cr_core::session::{self, Session};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub const POSTER_PX: u32 = 360;
pub const WIDE_PX: u32 = 640;
pub const HERO_PX: u32 = 1920;

pub struct App {
    pub ui: slint::Weak<AppWindow>,
    pub rt: tokio::runtime::Handle,
    pub images: Images,
    session: RefCell<Option<Arc<Session>>>,

    heroes: Rc<VecModel<HeroData>>,
    rows: Rc<VecModel<RowData>>,
    search_results: Rc<VecModel<CardData>>,
    popular: Rc<VecModel<CardData>>,
    browse_items: Rc<VecModel<CardData>>,
    simulcast_items: Rc<VecModel<CardData>>,
    calendar: Rc<VecModel<CalendarDayData>>,
    watchlist: Rc<VecModel<CardData>>,
    lists: Rc<VecModel<ListData>>,
    list_items: Rc<VecModel<CardData>>,
    history: Rc<VecModel<CardData>>,
    profiles: Rc<VecModel<ProfileData>>,
    seasons: Rc<VecModel<SeasonData>>,
    episodes: Rc<VecModel<EpisodeData>>,

    loaded: RefCell<HashSet<&'static str>>,
    search_gen: Cell<u64>,
    detail_gen: Cell<u64>,
    browse_gen: Cell<u64>,
    toast_gen: Cell<u64>,
    pub player: PlayerCtl,
    persist_timer: slint::Timer,
}

impl App {
    pub fn new(ui: &AppWindow, rt: tokio::runtime::Handle, images: Images) -> Rc<Self> {
        let app = Rc::new(App {
            ui: ui.as_weak(),
            rt: rt.clone(),
            images,
            session: RefCell::new(None),
            heroes: Rc::new(VecModel::default()),
            rows: Rc::new(VecModel::default()),
            search_results: Rc::new(VecModel::default()),
            popular: Rc::new(VecModel::default()),
            browse_items: Rc::new(VecModel::default()),
            simulcast_items: Rc::new(VecModel::default()),
            calendar: Rc::new(VecModel::default()),
            watchlist: Rc::new(VecModel::default()),
            lists: Rc::new(VecModel::default()),
            list_items: Rc::new(VecModel::default()),
            history: Rc::new(VecModel::default()),
            profiles: Rc::new(VecModel::default()),
            seasons: Rc::new(VecModel::default()),
            episodes: Rc::new(VecModel::default()),
            loaded: RefCell::new(HashSet::new()),
            search_gen: Cell::new(0),
            detail_gen: Cell::new(0),
            browse_gen: Cell::new(0),
            toast_gen: Cell::new(0),
            player: PlayerCtl::new(rt),
            persist_timer: slint::Timer::default(),
        });
        app.bind_models(ui);
        app
    }

    fn bind_models(&self, ui: &AppWindow) {
        ui.set_heroes(ModelRc::from(self.heroes.clone()));
        ui.set_rows(ModelRc::from(self.rows.clone()));
        ui.set_search_results(ModelRc::from(self.search_results.clone()));
        ui.set_popular(ModelRc::from(self.popular.clone()));
        ui.set_browse_items(ModelRc::from(self.browse_items.clone()));
        ui.set_simulcast_items(ModelRc::from(self.simulcast_items.clone()));
        ui.set_calendar(ModelRc::from(self.calendar.clone()));
        ui.set_watchlist(ModelRc::from(self.watchlist.clone()));
        ui.set_crunchylists(ModelRc::from(self.lists.clone()));
        ui.set_crunchylist_items(ModelRc::from(self.list_items.clone()));
        ui.set_history(ModelRc::from(self.history.clone()));
        ui.set_profiles(ModelRc::from(self.profiles.clone()));
        ui.set_seasons(ModelRc::from(self.seasons.clone()));
        ui.set_episodes(ModelRc::from(self.episodes.clone()));
        let genres: Vec<GenreData> = Session::genres()
            .into_iter()
            .map(|(slug, label)| GenreData { slug: slug.into(), label: label.into() })
            .collect();
        ui.set_genres(ModelRc::new(VecModel::from(genres)));
    }

    pub fn ui(&self) -> AppWindow {
        self.ui.upgrade().expect("janela fechada")
    }

    pub fn session(&self) -> Option<Arc<Session>> {
        self.session.borrow().clone()
    }

    /// Runs `fut` on tokio and `done` back on the UI thread.
    pub fn run<T: Send + 'static>(
        self: &Rc<Self>,
        fut: impl Future<Output = T> + Send + 'static,
        done: impl FnOnce(&Rc<App>, T) + 'static,
    ) {
        let handle = self.rt.spawn(fut);
        let weak = Rc::downgrade(self);
        let _ = slint::spawn_local(async move {
            match handle.await {
                Ok(v) => {
                    if let Some(app) = weak.upgrade() {
                        done(&app, v);
                    }
                }
                Err(e) => tracing::error!("tarefa falhou: {e}"),
            }
        });
    }

    /// Same as `run`, for API calls that need the session; does nothing when logged out.
    fn with_session<T: Send + 'static, F>(self: &Rc<Self>, make: impl FnOnce(Arc<Session>) -> F, done: impl FnOnce(&Rc<App>, T) + 'static)
    where
        F: Future<Output = T> + Send + 'static,
    {
        if let Some(s) = self.session() {
            self.run(make(s), done);
        }
    }

    pub fn toast(self: &Rc<Self>, msg: impl Into<SharedString>) {
        let ui = self.ui();
        ui.set_toast_message(msg.into());
        ui.set_toast_visible(true);
        let seq = self.toast_gen.get() + 1;
        self.toast_gen.set(seq);
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(Duration::from_millis(2800), move || {
            if let Some(app) = weak.upgrade() {
                if app.toast_gen.get() == seq {
                    app.ui().set_toast_visible(false);
                }
            }
        });
    }

    fn set_cards(&self, model: &Rc<VecModel<CardData>>, cards: &[MediaCard], wide: bool) {
        model.set_vec(cards.iter().map(convert::card).collect::<Vec<_>>());
        let urls = cards.iter().map(|c| convert::card_image(c, wide)).collect();
        self.images.fill(model, urls, if wide { WIDE_PX } else { POSTER_PX }, |c: &CardData| c.id.to_string(), |c, img| {
            c.image = img;
            c.has_image = true;
        });
    }

    // ------------------------------------------------------------ startup

    pub fn start(self: &Rc<Self>) {
        self.ui().set_screen("splash".into());
        if std::env::var("CR_ANON").is_ok() {
            return self.run(Session::anonymous(), |app, s| match s {
                Ok(s) => app.enter(Arc::new(s)),
                Err(e) => {
                    app.ui().set_login_error(e.into());
                    app.ui().set_screen("login".into());
                }
            });
        }
        self.run(Session::restore(), |app, s| match s {
            Some(s) => app.enter(Arc::new(s)),
            None => app.ui().set_screen("login".into()),
        });
    }

    pub fn login(self: &Rc<Self>, email: String, password: String) {
        let ui = self.ui();
        ui.set_login_busy(true);
        ui.set_login_error("".into());
        self.run(async move { Session::login(email.trim(), &password).await }, |app, r| {
            let ui = app.ui();
            ui.set_login_busy(false);
            match r {
                Ok(s) => {
                    let s = Arc::new(s);
                    *app.session.borrow_mut() = Some(s.clone());
                    app.run(async move { s.profiles().await }, |app, p| match p {
                        Ok(p) if p.len() > 1 => {
                            app.set_profiles(&p);
                            app.ui().set_screen("profiles".into());
                        }
                        _ => {
                            let s = app.session().unwrap();
                            app.enter(s);
                        }
                    });
                }
                Err(msg) => ui.set_login_error(msg.into()),
            }
        });
    }

    fn set_profiles(&self, p: &[cr_core::models::ProfileInfo]) {
        self.profiles.set_vec(p.iter().map(convert::profile).collect::<Vec<_>>());
        let urls = p.iter().map(|p| Some(p.avatar_url.clone())).collect();
        self.images.fill(&self.profiles, urls, 170, |p: &ProfileData| p.id.to_string(), |p, img| {
            p.avatar = img;
            p.has_avatar = true;
        });
        if let Some(sel) = p.iter().find(|p| p.selected).or(p.first()) {
            let ui = self.ui();
            ui.set_profile_name(sel.name.clone().into());
            let weak = self.ui.clone();
            self.images.load(sel.avatar_url.clone(), 170, move |img| {
                if let Some(ui) = weak.upgrade() {
                    ui.set_profile_avatar(img);
                    ui.set_profile_has_avatar(true);
                }
            });
        }
    }

    pub fn pick_profile(self: &Rc<Self>, id: String) {
        let current_selected = self.profiles.iter().any(|p| p.id == id && p.selected);
        if current_selected && self.ui().get_screen() == "profiles" {
            if let Some(s) = self.session() {
                return self.enter(s);
            }
        }
        if current_selected {
            return;
        }
        self.ui().set_screen("splash".into());
        self.with_session(move |s| async move { s.switch_profile(&id).await }, |app, r| match r {
            Ok(s) => app.enter(Arc::new(s)),
            Err(e) => {
                app.toast(format!("Não foi possível trocar de perfil: {e:#}"));
                if let Some(s) = app.session() {
                    app.enter(s);
                }
            }
        });
    }

    fn enter(self: &Rc<Self>, s: Arc<Session>) {
        *self.session.borrow_mut() = Some(s.clone());
        self.loaded.borrow_mut().clear();
        let ui = self.ui();
        ui.set_account_email(s.email.clone().into());
        ui.set_page("home".into());
        ui.set_detail_open(false);
        ui.set_screen("main".into());
        self.load_home(true);
        // Development shortcuts for screenshots/tests.
        if let Ok(page) = std::env::var("CR_DEV_PAGE") {
            self.navigate(page);
        }
        if let Ok(id) = std::env::var("CR_DEV_DETAIL") {
            self.open_detail(id);
        }
        self.with_session(|s| async move { s.profiles().await }, |app, p| {
            if let Ok(p) = p {
                app.set_profiles(&p);
            }
        });
        let weak = Rc::downgrade(self);
        self.persist_timer.start(slint::TimerMode::Repeated, Duration::from_secs(600), move || {
            if let Some(app) = weak.upgrade() {
                app.persist();
            }
        });
    }

    pub fn persist(self: &Rc<Self>) {
        self.with_session(|s| async move { s.persist().await }, |_, _| {});
    }

    pub fn logout(self: &Rc<Self>) {
        session::forget();
        *self.session.borrow_mut() = None;
        for m in [&self.search_results, &self.popular, &self.browse_items, &self.simulcast_items, &self.watchlist, &self.list_items, &self.history] {
            m.set_vec(vec![]);
        }
        self.rows.set_vec(vec![]);
        self.heroes.set_vec(vec![]);
        self.profiles.set_vec(vec![]);
        let ui = self.ui();
        ui.set_profile_has_avatar(false);
        ui.set_profile_name("".into());
        ui.set_screen("login".into());
    }

    // ---------------------------------------------------------- navigation

    pub fn navigate(self: &Rc<Self>, page: String) {
        let ui = self.ui();
        ui.set_detail_open(false);
        ui.set_page(page.clone().into());
        match page.as_str() {
            "home" => {}
            "search" => {
                ui.invoke_focus_search();
                if self.loaded.borrow_mut().insert("search") {
                    self.with_session(|s| async move { s.browse("popularity", "", 30).await }, |app, r| {
                        if let Ok(c) = r {
                            app.set_cards(&app.popular, &c, false);
                        }
                    });
                }
            }
            "browse" => {
                if self.loaded.borrow_mut().insert("browse") {
                    self.browse("popularity".into(), "".into());
                }
            }
            "simulcast" => {
                if self.loaded.borrow_mut().insert("simulcast") {
                    self.load_simulcasts();
                }
            }
            "mylist" => self.load_mylist(),
            "history" => self.load_history(),
            "account" => self.with_session(|s| async move { s.profiles().await }, |app, p| {
                if let Ok(p) = p {
                    app.set_profiles(&p);
                }
            }),
            _ => {}
        }
    }

    pub fn card_clicked(self: &Rc<Self>, c: CardData) {
        if c.kind == 2 {
            self.play(c.id.to_string(), false);
        } else {
            self.open_detail(c.id.to_string());
        }
    }

    pub fn escape(self: &Rc<Self>) {
        let ui = self.ui();
        if ui.get_fullscreen() {
            self.set_fullscreen(false);
        } else if ui.get_detail_open() {
            ui.set_detail_open(false);
        }
    }

    // ---------------------------------------------------------------- home

    pub fn load_home(self: &Rc<Self>, show_skeleton: bool) {
        if show_skeleton {
            self.ui().set_home_loading(true);
            self.rows.set_vec(vec![]);
            self.heroes.set_vec(vec![]);
        }
        self.with_session(|s| async move { s.home().await }, |app, r| {
            app.ui().set_home_loading(false);
            match r {
                Ok(h) => app.show_home(h),
                Err(e) => app.toast(format!("Não foi possível carregar o início: {e:#}")),
            }
        });
    }

    fn show_home(self: &Rc<Self>, h: HomeData) {
        self.heroes.set_vec(h.heroes.iter().map(convert::hero).collect::<Vec<_>>());
        let ui = self.ui();
        if ui.get_hero_index() as usize >= h.heroes.len() {
            ui.set_hero_index(0);
        }
        self.images.fill(&self.heroes, h.heroes.iter().map(|x| x.image.clone()).collect(), HERO_PX, |x: &HeroData| x.series_id.to_string(), |x, img| {
            x.image = img;
            x.has_image = true;
        });
        self.images.fill(&self.heroes, h.heroes.iter().map(|x| x.logo.clone()).collect(), 800, |x: &HeroData| x.series_id.to_string(), |x, img| {
            x.logo = img;
            x.has_logo = true;
        });
        let rows: Vec<RowData> = h
            .rows
            .iter()
            .map(|r| {
                let wide = r.style == cr_core::models::RowStyle::Wide;
                let model = Rc::new(VecModel::default());
                self.set_cards(&model, &r.items, wide);
                RowData { title: r.title.clone().into(), wide, items: ModelRc::from(model) }
            })
            .collect();
        self.rows.set_vec(rows);
    }

    // ------------------------------------------------------- search/browse

    pub fn search(self: &Rc<Self>, q: String) {
        let seq = self.search_gen.get() + 1;
        self.search_gen.set(seq);
        let ui = self.ui();
        if q.trim().is_empty() {
            ui.set_search_loading(false);
            self.search_results.set_vec(vec![]);
            return;
        }
        ui.set_search_loading(true);
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(Duration::from_millis(320), move || {
            let Some(app) = weak.upgrade() else { return };
            if app.search_gen.get() != seq {
                return;
            }
            app.with_session(move |s| async move { s.search(&q).await }, move |app, r| {
                if app.search_gen.get() != seq {
                    return;
                }
                app.ui().set_search_loading(false);
                match r {
                    Ok(c) => app.set_cards(&app.search_results, &c, false),
                    Err(e) => app.toast(format!("Erro na busca: {e:#}")),
                }
            });
        });
    }

    pub fn browse(self: &Rc<Self>, sort: String, genre: String) {
        let seq = self.browse_gen.get() + 1;
        self.browse_gen.set(seq);
        self.ui().set_browse_loading(true);
        self.with_session(move |s| async move { s.browse(&sort, &genre, 72).await }, move |app, r| {
            if app.browse_gen.get() != seq {
                return;
            }
            app.ui().set_browse_loading(false);
            match r {
                Ok(c) => app.set_cards(&app.browse_items, &c, false),
                Err(e) => app.toast(format!("Erro ao explorar: {e:#}")),
            }
        });
    }

    fn load_simulcasts(self: &Rc<Self>) {
        self.ui().set_simulcast_loading(true);
        self.with_session(|s| async move { tokio::join!(s.simulcasts(), s.calendar()) }, |app, (sim, cal)| {
            let ui = app.ui();
            ui.set_simulcast_loading(false);
            match sim {
                Ok((title, cards)) => {
                    ui.set_simulcast_title(title.into());
                    app.set_cards(&app.simulcast_items, &cards, false);
                }
                Err(e) => app.toast(format!("Erro nos simulcasts: {e:#}")),
            }
            if let Ok(days) = cal {
                let today = Session::today_index();
                let mut out = Vec::new();
                for (i, d) in days.iter().enumerate() {
                    let (data, items) = convert::calendar_day(d, i == today);
                    let urls = d.items.iter().map(|x| x.thumb.clone()).collect();
                    app.images.fill(&items, urls, 320, |x: &crate::CalendarItemData| x.episode_id.to_string(), |x, img| {
                        x.thumb = img;
                        x.has_thumb = true;
                    });
                    out.push(data);
                }
                app.calendar.set_vec(out);
                ui.set_calendar_day(today as i32);
            }
        });
    }

    // --------------------------------------------------------------- lists

    fn load_mylist(self: &Rc<Self>) {
        let first = self.loaded.borrow_mut().insert("mylist");
        if first {
            self.ui().set_mylist_loading(true);
        }
        self.with_session(|s| async move { tokio::join!(s.watchlist(), s.crunchylists()) }, |app, (wl, lists)| {
            app.ui().set_mylist_loading(false);
            match wl {
                Ok(c) => app.set_cards(&app.watchlist, &c, false),
                Err(e) => app.toast(format!("Erro na lista: {e:#}")),
            }
            if let Ok(l) = lists {
                app.lists.set_vec(l.iter().map(convert::list).collect::<Vec<_>>());
            }
        });
    }

    pub fn select_list(self: &Rc<Self>, id: String) {
        let title = self.lists.iter().find(|l| l.id == id).map(|l| l.title).unwrap_or_default();
        self.ui().set_crunchylist_title(title);
        self.list_items.set_vec(vec![]);
        self.with_session(move |s| async move { s.crunchylist_items(&id).await }, |app, r| match r {
            Ok(c) => app.set_cards(&app.list_items, &c, false),
            Err(e) => app.toast(format!("Erro ao abrir a lista: {e:#}")),
        });
    }

    pub fn create_list(self: &Rc<Self>, name: String) {
        self.with_session(move |s| async move { s.create_crunchylist(&name).await }, |app, r| match r {
            Ok(()) => {
                app.toast("Crunchylist criada");
                app.load_mylist();
            }
            Err(e) => app.toast(format!("Não foi possível criar: {e:#}")),
        });
    }

    pub fn delete_list(self: &Rc<Self>, id: String) {
        self.with_session(move |s| async move { s.delete_crunchylist(&id).await }, |app, r| match r {
            Ok(()) => {
                app.toast("Crunchylist apagada");
                app.load_mylist();
            }
            Err(e) => app.toast(format!("Não foi possível apagar: {e:#}")),
        });
    }

    fn load_history(self: &Rc<Self>) {
        if self.loaded.borrow_mut().insert("history") {
            self.ui().set_history_loading(true);
        }
        self.with_session(|s| async move { s.history(80).await }, |app, r| {
            app.ui().set_history_loading(false);
            match r {
                Ok(c) => app.set_cards(&app.history, &c, true),
                Err(e) => app.toast(format!("Erro no histórico: {e:#}")),
            }
        });
    }

    pub fn clear_history(self: &Rc<Self>) {
        self.with_session(|s| async move { s.clear_history().await }, |app, r| match r {
            Ok(()) => {
                app.history.set_vec(vec![]);
                app.toast("Histórico apagado");
                app.load_home(false);
            }
            Err(e) => app.toast(format!("Não foi possível limpar: {e:#}")),
        });
    }

    // -------------------------------------------------------------- detail

    pub fn open_detail(self: &Rc<Self>, id: String) {
        let seq = self.detail_gen.get() + 1;
        self.detail_gen.set(seq);
        let ui = self.ui();
        ui.set_detail(Default::default());
        ui.set_detail_loading(true);
        ui.set_episodes_loading(true);
        ui.set_detail_open(true);
        self.seasons.set_vec(vec![]);
        self.episodes.set_vec(vec![]);
        self.with_session(
            move |s| async move {
                let (d, up) = tokio::join!(s.detail(&id), s.up_next(&id));
                (d, up.ok().flatten())
            },
            move |app, (d, up)| {
                if app.detail_gen.get() != seq {
                    return;
                }
                let ui = app.ui();
                ui.set_detail_loading(false);
                let d = match d {
                    Ok(d) => d,
                    Err(e) => {
                        ui.set_detail_open(false);
                        return app.toast(format!("Não foi possível abrir: {e:#}"));
                    }
                };
                let mut data = convert::detail(&d);
                if let Some((ep, label)) = &up {
                    data.play_id = ep.clone().into();
                    data.play_label = label.clone().into();
                }
                ui.set_detail(data);
                let weak = app.ui.clone();
                if let Some(url) = d.image.clone() {
                    app.images.load(url, HERO_PX, move |img| {
                        if let Some(ui) = weak.upgrade() {
                            let mut x = ui.get_detail();
                            x.image = img;
                            x.has_image = true;
                            ui.set_detail(x);
                        }
                    });
                }
                app.seasons.set_vec(d.seasons.iter().map(convert::season).collect::<Vec<_>>());
                if let Some(first) = d.seasons.first() {
                    ui.set_season(first.id.clone().into());
                    app.load_episodes(first.id.clone());
                } else {
                    ui.set_episodes_loading(false);
                }
            },
        );
    }

    pub fn load_episodes(self: &Rc<Self>, season_id: String) {
        let seq = self.detail_gen.get();
        self.ui().set_episodes_loading(true);
        self.with_session(move |s| async move { s.episodes(&season_id).await }, move |app, r| {
            if app.detail_gen.get() != seq {
                return;
            }
            let ui = app.ui();
            ui.set_episodes_loading(false);
            match r {
                Ok(eps) => {
                    app.episodes.set_vec(eps.iter().map(convert::episode).collect::<Vec<_>>());
                    app.images.fill(&app.episodes, eps.iter().map(|e| e.thumb.clone()).collect(), WIDE_PX, |e: &EpisodeData| e.id.to_string(), |e, img| {
                        e.thumb = img;
                        e.has_thumb = true;
                    });
                    let mut d = ui.get_detail();
                    if d.play_id.is_empty() {
                        if let Some(first) = eps.first() {
                            d.play_id = first.id.clone().into();
                            d.play_label = format!("ASSISTIR {}", first.label).into();
                            ui.set_detail(d);
                        }
                    }
                }
                Err(e) => app.toast(format!("Erro nos episódios: {e:#}")),
            }
        });
    }

    pub fn toggle_watchlist(self: &Rc<Self>) {
        let ui = self.ui();
        let mut d = ui.get_detail();
        let add = !d.in_watchlist;
        d.in_watchlist = add;
        ui.set_detail(d.clone());
        let id = d.id.to_string();
        self.with_session(move |s| async move { s.set_in_watchlist(&id, add).await }, move |app, r| match r {
            Ok(()) => {
                app.toast(if add { "Adicionado à sua lista" } else { "Removido da sua lista" });
                app.loaded.borrow_mut().remove("mylist");
            }
            Err(e) => {
                let ui = app.ui();
                let mut d = ui.get_detail();
                d.in_watchlist = !add;
                ui.set_detail(d);
                app.toast(format!("Não foi possível atualizar a lista: {e:#}"));
            }
        });
    }

    pub fn rate(self: &Rc<Self>, stars: i32) {
        let id = self.ui().get_detail().id.to_string();
        self.with_session(move |s| async move { s.rate(&id, stars.clamp(1, 5) as u8).await }, |app, r| match r {
            Ok(()) => app.toast("Avaliação enviada"),
            Err(e) => app.toast(format!("Não foi possível avaliar: {e:#}")),
        });
    }

    // -------------------------------------------------------------- player

    /// `series`: the id is a series (hero button) and should resolve to its next episode.
    pub fn play(self: &Rc<Self>, id: String, series: bool) {
        let Some(s) = self.session() else { return };
        self.player.open(self, s, id, series);
    }

    pub fn set_fullscreen(self: &Rc<Self>, on: bool) {
        let ui = self.ui();
        ui.set_fullscreen(on);
        ui.window().set_fullscreen(on);
        self.player.update_bounds(self);
    }

    pub fn after_player_closed(self: &Rc<Self>) {
        // progress changed: refresh "continue watching" and the open detail page
        self.load_home(false);
        let ui = self.ui();
        if ui.get_detail_open() {
            let season = ui.get_season().to_string();
            if !season.is_empty() {
                self.load_episodes(season);
            }
        }
        self.loaded.borrow_mut().remove("history");
    }

    pub fn shutdown(self: &Rc<Self>) {
        let s = self.session();
        let pb = self.player.take_active();
        let pos = self.player.last_position();
        let _ = self.rt.block_on(async move {
            tokio::time::timeout(Duration::from_secs(4), async move {
                if let Some(pb) = pb {
                    if pos > 1.0 {
                        pb.save_progress(pos as u32).await;
                    }
                    pb.close().await;
                }
                if let Some(s) = s {
                    s.persist().await;
                }
            })
            .await
        });
    }
}
