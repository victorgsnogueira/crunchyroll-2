//! Plain data the UI shows. Nothing here talks to the network.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Series,
    Movie,
    Episode,
}

/// One tile in a row (poster or wide thumbnail).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaCard {
    pub id: String,
    pub kind: MediaKind,
    pub title: String,
    pub subtitle: String,
    /// 2:3 poster.
    pub poster: Option<String>,
    /// 16:9 image.
    pub wide: Option<String>,
    /// 0.0 ..= 1.0, only for episodes.
    pub progress: f32,
    /// Series (or movie listing) this card belongs to.
    pub parent_id: Option<String>,
    pub badge: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowStyle {
    Poster,
    Wide,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeedRow {
    pub title: String,
    pub style: RowStyle,
    pub items: Vec<MediaCard>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hero {
    pub series_id: String,
    pub title: String,
    pub description: String,
    pub image: Option<String>,
    pub logo: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeData {
    pub heroes: Vec<Hero>,
    pub rows: Vec<FeedRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeasonInfo {
    pub id: String,
    pub label: String,
    pub episode_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesDetail {
    pub id: String,
    pub kind: MediaKind,
    pub title: String,
    pub description: String,
    pub image: Option<String>,
    pub poster: Option<String>,
    pub meta_line: String,
    pub genres: Vec<String>,
    pub audio: Vec<String>,
    pub subtitles: Vec<String>,
    pub maturity: String,
    pub seasons: Vec<SeasonInfo>,
    pub in_watchlist: bool,
    pub rating: f64,
    pub rating_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EpisodeItem {
    pub id: String,
    pub label: String,
    pub title: String,
    pub description: String,
    pub duration_label: String,
    pub thumb: Option<String>,
    pub progress: f32,
    pub premium_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkipKind {
    Intro,
    Recap,
    Credits,
    Preview,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkipMarker {
    pub kind: SkipKind,
    pub start: f32,
    pub end: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub locale: String,
    pub label: String,
    pub url: String,
    pub format: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioVersion {
    pub media_id: String,
    pub locale: String,
    pub label: String,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NextEpisode {
    pub id: String,
    pub title: String,
    pub thumb: Option<String>,
}

/// Everything the player page needs to start a video.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackInfo {
    pub episode_id: String,
    pub title: String,
    pub subtitle: String,
    pub manifest_url: String,
    pub license_url: String,
    pub video_token: String,
    pub subtitles: Vec<SubtitleTrack>,
    pub audio_versions: Vec<AudioVersion>,
    pub skip: Vec<SkipMarker>,
    pub resume_at: u32,
    pub next: Option<NextEpisode>,
    pub preferred_subtitle: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub id: String,
    pub name: String,
    pub avatar_url: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalendarItem {
    pub series_id: String,
    pub episode_id: String,
    pub title: String,
    pub episode_label: String,
    pub time_label: String,
    pub thumb: Option<String>,
    pub premium: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalendarDay {
    pub label: String,
    pub items: Vec<CalendarItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListInfo {
    pub id: String,
    pub title: String,
    pub count: u32,
}
