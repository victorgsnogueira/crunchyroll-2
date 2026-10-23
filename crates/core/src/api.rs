//! Screen-level data loading on top of crunchyroll-rs.

use crate::cards::{self, HERO_W, WIDE_W, collection_card, episode_card, genre_name, movie_card, series_card};
use crate::format;
use crate::images::pick;
use crate::locale::language_name;
use crate::manifest::pin_base_url;
use crate::models::*;
use crate::session::Session;
use anyhow::{Context, Result, anyhow};
use chrono::{Datelike, Local, Utc};
use crunchyroll_rs::feed::HomeFeed;
use crunchyroll_rs::list::WatchlistOptions;
use crunchyroll_rs::media::{SkipEvents, Stream};
use crunchyroll_rs::search::{BrowseOptions, BrowseSortType};
use crunchyroll_rs::{Episode, Locale, MediaCollection, Movie, MovieListing, Season, Series};
use futures_util::StreamExt;
use futures_util::future::join_all;
use serde::Deserialize;
use std::collections::HashMap;

const ROW_LEN: usize = 20;

/// Collects up to `n` successful items of a paginated stream.
async fn take<T, S>(stream: S, n: usize) -> Vec<T>
where
    S: futures_util::Stream<Item = std::result::Result<T, crunchyroll_rs::Error>> + Unpin,
{
    stream
        .take(n)
        .filter_map(|r| async move {
            if let Err(e) = &r {
                tracing::warn!("item ignorado: {e}");
            }
            r.ok()
        })
        .collect()
        .await
}

impl Session {
    // ---------------------------------------------------------------- home

    pub async fn home(&self) -> Result<HomeData> {
        let feed: Vec<HomeFeed> = take(self.cr.home_feed(), 30).await;
        let mut heroes = Vec::new();
        let mut row_futures = Vec::new();
        let mut similar_used = 0;
        let mut series_feeds_used = 0;

        for item in feed {
            match item {
                HomeFeed::CarouselFeed(items) => {
                    for c in items {
                        let series_id = match &c.panel {
                            Some(MediaCollection::Series(s)) => s.id.clone(),
                            Some(MediaCollection::Episode(e)) => e.series_id.clone(),
                            Some(MediaCollection::MovieListing(m)) => m.id.clone(),
                            _ => match crunchyroll_rs::parse_url(&c.link) {
                                Some(crunchyroll_rs::UrlType::Series(id)) => id,
                                Some(crunchyroll_rs::UrlType::MovieListing(id)) => id,
                                _ => continue,
                            },
                        };
                        heroes.push(Hero {
                            series_id,
                            title: c.title,
                            description: c.description,
                            image: c.images.landscape_large,
                            logo: c.images.logo,
                        });
                    }
                }
                HomeFeed::History => row_futures.push(Box::pin(self.continue_row()) as RowFuture),
                HomeFeed::Watchlist => row_futures.push(Box::pin(self.watchlist_row())),
                HomeFeed::Recommendation => row_futures.push(Box::pin(self.recommendation_row())),
                HomeFeed::SeriesFeed(f) if series_feeds_used < 6 => {
                    series_feeds_used += 1;
                    row_futures.push(Box::pin(self.series_feed_row(f.title, f.ids)));
                }
                HomeFeed::SimilarTo(f) if similar_used < 2 => {
                    similar_used += 1;
                    row_futures.push(Box::pin(self.similar_row(f.title, f.similar_id)));
                }
                _ => {}
            }
        }

        let rows = join_all(row_futures)
            .await
            .into_iter()
            .filter_map(|r| match r {
                Ok(row) if !row.items.is_empty() => Some(row),
                Ok(_) => None,
                Err(e) => {
                    tracing::warn!("fileira ignorada: {e:#}");
                    None
                }
            })
            .collect();
        Ok(HomeData { heroes, rows })
    }

    async fn continue_row(&self) -> Result<FeedRow> {
        Ok(FeedRow { title: "Continuar assistindo".into(), style: RowStyle::Wide, items: self.history(ROW_LEN).await? })
    }

    async fn watchlist_row(&self) -> Result<FeedRow> {
        Ok(FeedRow { title: "Minha lista".into(), style: RowStyle::Poster, items: self.watchlist().await? })
    }

    async fn recommendation_row(&self) -> Result<FeedRow> {
        let items: Vec<MediaCollection> = take(self.cr.recommendations(), ROW_LEN).await;
        Ok(FeedRow {
            title: "Recomendados para você".into(),
            style: RowStyle::Poster,
            items: items.iter().filter_map(|c| collection_card(c, 0, false)).collect(),
        })
    }

    async fn series_feed_row(&self, title: String, ids: Vec<String>) -> Result<FeedRow> {
        let series = join_all(ids.iter().take(ROW_LEN).map(|id| self.cr.media_from_id::<Series>(id))).await;
        Ok(FeedRow {
            title,
            style: RowStyle::Poster,
            items: series.into_iter().filter_map(|s| s.ok()).map(|s| series_card(&s)).collect(),
        })
    }

    async fn similar_row(&self, title: String, id: String) -> Result<FeedRow> {
        let base = self.cr.media_collection_from_id(&id).await?;
        let series = match base {
            MediaCollection::Series(s) => s,
            MediaCollection::Episode(e) => e.series().await?,
            _ => return Ok(FeedRow { title, style: RowStyle::Poster, items: vec![] }),
        };
        let similar = take(series.similar(), ROW_LEN).await;
        Ok(FeedRow {
            title,
            style: RowStyle::Poster,
            items: similar.into_iter().filter_map(|c| collection_card(&c.into(), 0, false)).collect(),
        })
    }

    // -------------------------------------------------------------- lists

    pub async fn history(&self, n: usize) -> Result<Vec<MediaCard>> {
        let entries = take(self.cr.watch_history(), n).await;
        let mut seen = std::collections::HashSet::new();
        Ok(entries
            .iter()
            .filter_map(|h| {
                let card = collection_card(h.panel.as_ref()?, h.playhead, h.fully_watched)?;
                // one entry per series, newest first
                seen.insert(card.parent_id.clone().unwrap_or_else(|| card.id.clone())).then_some(card)
            })
            .collect())
    }

    pub async fn clear_history(&self) -> Result<()> {
        Ok(self.cr.clear_watch_history().await?)
    }

    pub async fn watchlist(&self) -> Result<Vec<MediaCard>> {
        let entries = self.cr.watchlist(WatchlistOptions::default()).await?;
        Ok(entries
            .iter()
            .filter_map(|w| {
                let panel = match &w.panel {
                    // the watchlist panel is the next episode; show its series instead
                    MediaCollection::Episode(e) => {
                        let mut c = episode_card(e, w.playhead, w.fully_watched);
                        c.kind = MediaKind::Series;
                        c.id = e.series_id.clone();
                        c.subtitle = if w.new { "Novo episódio".into() } else { c.subtitle };
                        return Some(c);
                    }
                    other => other,
                };
                collection_card(panel, 0, false)
            })
            .collect())
    }

    pub async fn set_in_watchlist(&self, id: &str, add: bool) -> Result<()> {
        let c = self.cr.media_collection_from_id(id).await?;
        match (c, add) {
            (MediaCollection::Series(s), true) => s.add_to_watchlist().await?,
            (MediaCollection::MovieListing(m), true) => m.add_to_watchlist().await?,
            (MediaCollection::Series(s), false) => {
                if let Some(e) = s.into_watchlist_entry().await? {
                    e.remove().await?
                }
            }
            (MediaCollection::MovieListing(m), false) => {
                if let Some(e) = m.into_watchlist_entry().await? {
                    e.remove().await?
                }
            }
            _ => return Err(anyhow!("só séries e filmes entram na lista")),
        }
        Ok(())
    }

    pub async fn crunchylists(&self) -> Result<Vec<ListInfo>> {
        let lists = self.cr.crunchylists().await?;
        Ok(lists
            .items
            .iter()
            .map(|l| ListInfo { id: l.list_id.clone(), title: l.title.clone(), count: l.total })
            .collect())
    }

    pub async fn crunchylist_items(&self, list_id: &str) -> Result<Vec<MediaCard>> {
        let lists = self.cr.crunchylists().await?;
        let preview = lists.items.iter().find(|l| l.list_id == list_id).context("lista não encontrada")?;
        let list = preview.crunchylist().await?;
        Ok(list.items.iter().filter_map(|i| collection_card(&i.panel, 0, false)).collect())
    }

    pub async fn create_crunchylist(&self, title: &str) -> Result<()> {
        self.cr.crunchylists().await?.create(title).await?;
        Ok(())
    }

    pub async fn delete_crunchylist(&self, list_id: &str) -> Result<()> {
        let lists = self.cr.crunchylists().await?;
        let preview = lists.items.into_iter().find(|l| l.list_id == list_id).context("lista não encontrada")?;
        preview.crunchylist().await?.delete().await?;
        Ok(())
    }

    pub async fn add_to_crunchylist(&self, list_id: &str, media_id: &str) -> Result<()> {
        let lists = self.cr.crunchylists().await?;
        let preview = lists.items.iter().find(|l| l.list_id == list_id).context("lista não encontrada")?;
        let media = self.cr.media_collection_from_id(media_id).await?;
        preview.crunchylist().await?.add(media).await?;
        Ok(())
    }

    // ------------------------------------------------------------- browse

    pub async fn search(&self, query: &str) -> Result<Vec<MediaCard>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }
        let results = self.cr.query(query.trim());
        let items = take(results.top_results, 40).await;
        Ok(items.into_iter().filter_map(|c| collection_card(&c.into(), 0, false)).collect())
    }

    /// sort: "popularity" | "newly_added" | "alphabetical"; genre: slug or "".
    pub async fn browse(&self, sort: &str, genre: &str, n: usize) -> Result<Vec<MediaCard>> {
        let sort = match sort {
            "newly_added" => BrowseSortType::NewlyAdded,
            "alphabetical" => BrowseSortType::Alphabetical,
            _ => BrowseSortType::Popularity,
        };
        let mut opts = BrowseOptions::default().sort(sort);
        if !genre.is_empty() {
            if let Ok(cat) = serde_json::from_value(serde_json::Value::String(genre.to_string())) {
                opts = opts.categories(vec![cat]);
            }
        }
        let items = take(self.cr.browse(opts), n).await;
        Ok(items.into_iter().filter_map(|c| collection_card(&c.into(), 0, false)).collect())
    }

    pub fn genres() -> Vec<(String, String)> {
        [
            "action", "adventure", "comedy", "drama", "fantasy", "music", "romance", "sci-fi", "seinen", "shojo",
            "shonen", "slice-of-life", "sports", "supernatural", "thriller",
        ]
        .iter()
        .map(|s| (s.to_string(), genre_name(s)))
        .collect()
    }

    /// Current simulcast season title and its shows.
    pub async fn simulcasts(&self) -> Result<(String, Vec<MediaCard>)> {
        let seasons = self.cr.simulcast_seasons(Locale::pt_BR).await?;
        let season = seasons.first().context("nenhuma temporada de simulcast")?;
        let opts = BrowseOptions::default().simulcast_season(season.id.clone()).sort(BrowseSortType::Popularity);
        let items = take(self.cr.browse(opts), 80).await;
        Ok((
            season.localization.title.clone(),
            items.into_iter().filter_map(|c| collection_card(&c.into(), 0, false)).collect(),
        ))
    }

    pub async fn calendar(&self) -> Result<Vec<CalendarDay>> {
        let week = self.cr.release_calendar(Utc::now()).await?;
        let days = [week.monday, week.tuesday, week.wednesday, week.thursday, week.friday, week.saturday, week.sunday];
        Ok(days
            .into_iter()
            .enumerate()
            .map(|(i, items)| CalendarDay {
                label: format::weekday_pt(i).into(),
                items: items
                    .into_iter()
                    .map(|it| {
                        let local = it.release_time.with_timezone(&Local);
                        CalendarItem {
                            series_id: it.series_id,
                            episode_id: it.episode_id,
                            title: it.season_title,
                            episode_label: format!("Episódio {}", it.episode_number),
                            time_label: local.format("%H:%M").to_string(),
                            thumb: (!it.thumbnail_url.is_empty()).then_some(it.thumbnail_url),
                            premium: it.premium,
                        }
                    })
                    .collect(),
            })
            .collect())
    }

    /// Index of today in the calendar (0 = Monday).
    pub fn today_index() -> usize {
        Local::now().weekday().num_days_from_monday() as usize
    }

    // ------------------------------------------------------------- detail

    pub async fn detail(&self, id: &str) -> Result<SeriesDetail> {
        match self.cr.media_collection_from_id(id).await? {
            MediaCollection::Series(s) => self.series_detail(s).await,
            MediaCollection::MovieListing(m) => self.movie_detail(m).await,
            MediaCollection::Episode(e) => self.series_detail(e.series().await?).await,
            MediaCollection::Season(s) => self.series_detail(s.series().await?).await,
            _ => Err(anyhow!("conteúdo não suportado")),
        }
    }

    async fn series_detail(&self, s: Series) -> Result<SeriesDetail> {
        let (seasons, wl, rating) = tokio::join!(s.seasons(), s.into_watchlist_entry(), s.rating());
        let seasons = seasons?;
        let rating = rating.ok();
        let mut meta = Vec::new();
        if let Some(y) = s.series_launch_year {
            meta.push(y.to_string());
        }
        if s.season_count > 0 {
            meta.push(format!("{} temporada{}", s.season_count, if s.season_count > 1 { "s" } else { "" }));
        }
        if s.episode_count > 0 {
            meta.push(format!("{} episódios", s.episode_count));
        }
        Ok(SeriesDetail {
            id: s.id.clone(),
            kind: MediaKind::Series,
            title: s.title.clone(),
            description: if s.extended_description.is_empty() { s.description.clone() } else { s.extended_description.clone() },
            image: pick(&s.images.poster_wide, HERO_W),
            poster: pick(&s.images.poster_tall, cards::POSTER_W),
            meta_line: meta.join(" · "),
            genres: s.categories.iter().flatten().map(|c| genre_name(&c.to_string())).collect(),
            audio: dedup(s.audio_locales.iter().map(|l| language_name(&l.to_string()))),
            subtitles: dedup(s.subtitle_locales.iter().map(|l| language_name(&l.to_string()))),
            maturity: s.maturity_ratings.first().cloned().unwrap_or_default(),
            seasons: seasons.iter().map(season_info).collect(),
            in_watchlist: matches!(wl, Ok(Some(_))),
            rating: rating.as_ref().map(|r| r.average).unwrap_or(0.0),
            rating_count: rating.as_ref().map(|r| r.total).unwrap_or(0),
        })
    }

    async fn movie_detail(&self, m: MovieListing) -> Result<SeriesDetail> {
        let wl = m.into_watchlist_entry().await;
        Ok(SeriesDetail {
            id: m.id.clone(),
            kind: MediaKind::Movie,
            title: m.title.clone(),
            description: if m.extended_description.is_empty() { m.description.clone() } else { m.extended_description.clone() },
            image: pick(&m.images.poster_wide, HERO_W),
            poster: pick(&m.images.poster_tall, cards::POSTER_W),
            meta_line: if m.movie_release_year > 0 { format!("Filme · {}", m.movie_release_year) } else { "Filme".into() },
            genres: m.categories.iter().flatten().map(|c| genre_name(&c.to_string())).collect(),
            audio: m.audio_locale.iter().map(|l| language_name(&l.to_string())).collect(),
            subtitles: dedup(m.subtitle_locales.iter().map(|l| language_name(&l.to_string()))),
            maturity: m.maturity_ratings.first().cloned().unwrap_or_default(),
            seasons: vec![SeasonInfo { id: m.id.clone(), label: "Filme".into(), episode_count: 1 }],
            in_watchlist: matches!(wl, Ok(Some(_))),
            rating: 0.0,
            rating_count: 0,
        })
    }

    /// Episodes of a season (or the movies of a movie listing), with watch progress.
    pub async fn episodes(&self, season_or_listing_id: &str) -> Result<Vec<EpisodeItem>> {
        match self.cr.media_collection_from_id(season_or_listing_id).await? {
            MediaCollection::MovieListing(m) => {
                let movies = m.movies().await?;
                let ids: Vec<String> = movies.iter().map(|m| m.id.clone()).collect();
                let heads = self.playheads(&ids).await.unwrap_or_default();
                Ok(movies.iter().map(|m| movie_item(m, heads.get(&m.id))).collect())
            }
            _ => {
                let season: Season = self.cr.media_from_id(season_or_listing_id).await?;
                let eps = season.episodes().await?;
                let ids: Vec<String> = eps.iter().map(|e| e.id.clone()).collect();
                let heads = self.playheads(&ids).await.unwrap_or_default();
                Ok(eps.iter().map(|e| episode_item(e, heads.get(&e.id))).collect())
            }
        }
    }

    /// Bulk playheads (one request for a whole season).
    pub async fn playheads(&self, ids: &[String]) -> Result<HashMap<String, (u32, bool)>> {
        #[derive(Deserialize)]
        struct Item {
            content_id: String,
            playhead: u32,
            #[serde(default)]
            fully_watched: bool,
        }
        #[derive(Deserialize)]
        struct Resp {
            data: Vec<Item>,
        }
        let mut out = HashMap::new();
        for chunk in ids.chunks(50) {
            let url = format!("https://www.crunchyroll.com/content/v2/{}/playheads", self.account_id);
            let resp: Resp = self
                .cr
                .client()
                .get(url)
                .bearer_auth(self.access_token().await)
                .query(&[("content_ids", chunk.join(",")), ("locale", "pt-BR".into())])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            out.extend(resp.data.into_iter().map(|i| (i.content_id, (i.playhead, i.fully_watched))));
        }
        Ok(out)
    }

    pub async fn rate(&self, series_id: &str, stars: u8) -> Result<()> {
        use crunchyroll_rs::media::RatingStar;
        let star = match stars {
            1 => RatingStar::OneStar,
            2 => RatingStar::TwoStars,
            3 => RatingStar::ThreeStars,
            4 => RatingStar::FourStars,
            _ => RatingStar::FiveStars,
        };
        let s: Series = self.cr.media_from_id(series_id).await?;
        s.rate(star).await?;
        Ok(())
    }

    // ----------------------------------------------------------- playback

    /// Opens a stream. Call `Playback::close` when done to free the stream slot.
    pub async fn play(&self, media_id: &str) -> Result<Playback> {
        let media = self.cr.media_collection_from_id(media_id).await?;
        let video = match media {
            MediaCollection::Episode(e) => Video::Episode(Box::new(e)),
            MediaCollection::Movie(m) => Video::Movie(Box::new(m)),
            MediaCollection::Series(s) => {
                let first = s.seasons().await?.into_iter().next().context("série sem temporadas")?;
                let ep = first.episodes().await?.into_iter().next().context("temporada vazia")?;
                Video::Episode(Box::new(ep))
            }
            _ => return Err(anyhow!("este item não pode ser reproduzido")),
        };
        let (stream, skip, head, next) = match &video {
            Video::Episode(e) => {
                let (st, sk, ph, nx) = tokio::join!(e.stream(), e.skip_events(), e.playhead(), e.next());
                let next = nx.ok().flatten().map(|n| NextEpisode {
                    id: n.media.id.clone(),
                    title: format!(
                        "{} · {}",
                        format::episode_label(n.media.season_number, n.media.episode_number, &n.media.episode),
                        n.media.title
                    ),
                    thumb: pick(&n.media.images, WIDE_W),
                });
                (st, sk, ph, next)
            }
            Video::Movie(m) => {
                let (st, sk, ph) = tokio::join!(m.stream(), m.skip_events(), m.playhead());
                (st, sk, ph, None)
            }
        };
        let stream = stream.map_err(|e| anyhow!(stream_error(&e)))?;
        let (playhead, watched) = head.ok().flatten().map(|h| (h.playhead, h.fully_watched)).unwrap_or((0, false));
        let info = build_playback_info(&video, &stream, skip.ok().flatten(), playhead, watched, next);
        Ok(Playback { video, stream: Some(stream), info, account_id: self.account_id.clone() })
    }

    /// The DASH manifest with segment URLs pinned to the CDN.
    pub async fn manifest(&self, p: &Playback) -> Result<String> {
        let stream = p.stream.as_ref().context("stream fechado")?;
        let body = self
            .cr
            .client()
            .get(&stream.url)
            .bearer_auth(self.access_token().await)
            .query(&[("accountid", p.account_id.as_str()), ("playbackGuid", stream.token.as_str())])
            .send()
            .await?
            .error_for_status()
            .context("manifesto recusado")?
            .text()
            .await?;
        Ok(pin_base_url(&body, &stream.url))
    }

    /// Forwards a CDM license challenge to Crunchyroll's license proxy, untouched.
    pub async fn license(&self, p: &Playback, system: DrmSystem, challenge: Vec<u8>) -> Result<Vec<u8>> {
        let stream = p.stream.as_ref().context("stream fechado")?;
        let url = license_url(&stream.drm.drm_url, system);
        let resp = self
            .cr
            .client()
            .post(&url)
            .bearer_auth(self.access_token().await)
            .header("Content-Type", "application/octet-stream")
            .header("X-Cr-Content-Id", p.info.episode_id.as_str())
            .header("X-Cr-Video-Token", stream.token.as_str())
            .body(challenge)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?.to_vec();
        if !status.is_success() {
            return Err(anyhow!("licença recusada ({status}): {}", String::from_utf8_lossy(&bytes)));
        }
        Ok(bytes)
    }

    /// Opens another audio version of the same episode, closing the current stream first.
    pub async fn switch_audio(&self, current: Playback, version_media_id: &str) -> Result<Playback> {
        current.close().await;
        self.play(version_media_id).await
    }
}

type RowFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<FeedRow>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrmSystem {
    Widevine,
    PlayReady,
}

pub fn license_url(drm_url: &str, system: DrmSystem) -> String {
    let base = if drm_url.is_empty() {
        "https://www.crunchyroll.com/license/v1/license/widevine".to_string()
    } else {
        drm_url.to_string()
    };
    let (path, query) = base.split_once('?').map(|(a, b)| (a.to_string(), Some(b.to_string()))).unwrap_or((base, None));
    let path = match system {
        DrmSystem::Widevine => path,
        DrmSystem::PlayReady => match path.rfind('/') {
            Some(i) => format!("{}/playReady", &path[..i]),
            None => path,
        },
    };
    let mut q: Vec<String> = query.map(|q| q.split('&').map(String::from).collect()).unwrap_or_default();
    if !q.iter().any(|p| p.starts_with("specConform=")) {
        q.push("specConform=true".into());
    }
    format!("{path}?{}", q.join("&"))
}

pub enum Video {
    Episode(Box<Episode>),
    Movie(Box<Movie>),
}

pub struct Playback {
    video: Video,
    stream: Option<Stream>,
    pub info: PlaybackInfo,
    account_id: String,
}

impl Playback {
    pub async fn save_progress(&self, secs: u32) {
        let r = match &self.video {
            Video::Episode(e) => e.set_playhead(secs).await,
            Video::Movie(m) => m.set_playhead(secs).await,
        };
        if let Err(e) = r {
            tracing::warn!("progresso não salvo: {e}");
        }
    }

    /// Frees the stream slot on Crunchyroll's side.
    pub async fn close(mut self) {
        if let Some(s) = self.stream.take() {
            if let Err(e) = s.invalidate().await {
                tracing::warn!("stream não liberado: {e}");
            }
        }
    }
}

fn stream_error(e: &crunchyroll_rs::Error) -> String {
    let s = e.to_string();
    if s.contains("TOO_MANY_ACTIVE_STREAMS") || s.contains("4294") {
        "Você já está assistindo em outros aparelhos. Feche um deles e tente de novo.".into()
    } else if s.contains("premium") || s.contains("403") {
        "Este episódio exige Crunchyroll Premium ou não está disponível na sua região.".into()
    } else {
        format!("Não foi possível abrir o vídeo: {s}")
    }
}

fn build_playback_info(
    video: &Video,
    stream: &Stream,
    skip: Option<SkipEvents>,
    playhead: u32,
    watched: bool,
    next: Option<NextEpisode>,
) -> PlaybackInfo {
    let (id, title, subtitle, duration) = match video {
        Video::Episode(e) => (
            e.id.clone(),
            e.series_title.clone(),
            format!("{} · {}", format::episode_label(e.season_number, e.episode_number, &e.episode), e.title),
            e.duration.num_seconds().max(0) as u64,
        ),
        Video::Movie(m) => (m.id.clone(), m.movie_listing_title.clone(), m.title.clone(), m.duration.num_seconds().max(0) as u64),
    };
    let current_audio = stream.audio_locale.to_string();
    PlaybackInfo {
        episode_id: id,
        title,
        subtitle,
        manifest_url: stream.url.clone(),
        license_url: stream.drm.drm_url.clone(),
        video_token: stream.token.clone(),
        subtitles: stream
            .subtitles
            .iter()
            .filter(|s| !s.url.is_empty())
            .map(|s| SubtitleTrack {
                locale: s.locale.to_string(),
                label: language_name(&s.locale.to_string()),
                url: s.url.clone(),
                format: s.format.clone(),
            })
            .collect(),
        audio_versions: stream
            .versions
            .iter()
            .map(|v| AudioVersion {
                media_id: v.id.clone(),
                locale: v.audio_locale.to_string(),
                label: language_name(&v.audio_locale.to_string()),
                current: v.audio_locale.to_string() == current_audio,
            })
            .collect(),
        skip: skip.map(skip_markers).unwrap_or_default(),
        resume_at: format::resume_position(playhead, duration, watched),
        next,
        preferred_subtitle: if current_audio == "pt-BR" { None } else { Some("pt-BR".into()) },
    }
}

pub fn skip_markers(s: SkipEvents) -> Vec<SkipMarker> {
    let mut out = Vec::new();
    for (kind, ev) in [
        (SkipKind::Recap, s.recap),
        (SkipKind::Intro, s.intro),
        (SkipKind::Credits, s.credits),
        (SkipKind::Preview, s.preview),
    ] {
        if let Some(ev) = ev {
            if ev.end > ev.start {
                out.push(SkipMarker { kind, start: ev.start, end: ev.end });
            }
        }
    }
    out
}

fn season_info(s: &Season) -> SeasonInfo {
    let audio = s.audio_locales.first().map(|l| l.to_string()).unwrap_or_default();
    let label = if s.title.is_empty() { format!("Temporada {}", s.season_number) } else { s.title.clone() };
    let label = if s.is_dubbed && !s.is_subbed && !audio.is_empty() && audio != "ja-JP" {
        format!("{label} ({})", language_name(&audio))
    } else {
        label
    };
    SeasonInfo { id: s.id.clone(), label, episode_count: s.number_of_episodes }
}

fn episode_item(e: &Episode, head: Option<&(u32, bool)>) -> EpisodeItem {
    let (ph, fw) = head.copied().unwrap_or((0, false));
    let dur = e.duration.num_seconds().max(0) as u64;
    EpisodeItem {
        id: e.id.clone(),
        label: format::episode_label(e.season_number, e.episode_number, &e.episode),
        title: e.title.clone(),
        description: e.description.clone(),
        duration_label: format::duration_label(dur),
        thumb: pick(&e.images, WIDE_W),
        progress: format::progress(ph, dur, fw),
        premium_only: e.is_premium_only,
    }
}

fn movie_item(m: &Movie, head: Option<&(u32, bool)>) -> EpisodeItem {
    let (ph, fw) = head.copied().unwrap_or((0, false));
    let dur = m.duration.num_seconds().max(0) as u64;
    let c = movie_card(m, ph, fw);
    EpisodeItem {
        id: m.id.clone(),
        label: "Filme".into(),
        title: m.title.clone(),
        description: m.description.clone(),
        duration_label: format::duration_label(dur),
        thumb: c.wide,
        progress: c.progress,
        premium_only: m.is_premium_only,
    }
}

fn dedup(it: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in it {
        if !v.contains(&s) {
            v.push(s);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn license_url_adds_spec_conform_and_switches_system() {
        assert_eq!(
            license_url("https://lic.x/v1/license/widevine", DrmSystem::Widevine),
            "https://lic.x/v1/license/widevine?specConform=true"
        );
        assert_eq!(
            license_url("https://lic.x/v1/license/widevine?a=1", DrmSystem::PlayReady),
            "https://lic.x/v1/license/playReady?a=1&specConform=true"
        );
        assert_eq!(
            license_url("https://lic.x/v1/license/widevine?specConform=false", DrmSystem::Widevine),
            "https://lic.x/v1/license/widevine?specConform=false"
        );
        assert!(license_url("", DrmSystem::Widevine).starts_with("https://www.crunchyroll.com/license/v1/license/widevine"));
    }

    #[test]
    fn skip_markers_keep_valid_ranges_in_order() {
        let s: SkipEvents = serde_json::from_value(json!({
            "intro": {"start": 90.0, "end": 180.0, "type": "intro"},
            "credits": {"start": 1300.0, "end": 1390.0, "type": "credits"},
            "recap": {"start": 10.0, "end": 10.0, "type": "recap"}
        }))
        .unwrap();
        let m = skip_markers(s);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], SkipMarker { kind: SkipKind::Intro, start: 90.0, end: 180.0 });
        assert_eq!(m[1].kind, SkipKind::Credits);
    }

    #[test]
    fn dedup_keeps_first_order() {
        assert_eq!(dedup(["a", "b", "a"].iter().map(|s| s.to_string())), vec!["a", "b"]);
    }
}
