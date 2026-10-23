//! Turns crunchyroll-rs media into UI cards. Pure functions, unit tested with JSON.

use crate::format;
use crate::images::pick;
use crate::models::{MediaCard, MediaKind};
use crunchyroll_rs::{Episode, MediaCollection, Movie, MovieListing, Series};

pub const POSTER_W: u32 = 360;
pub const WIDE_W: u32 = 640;
pub const HERO_W: u32 = 1920;

pub fn series_card(s: &Series) -> MediaCard {
    MediaCard {
        id: s.id.clone(),
        kind: MediaKind::Series,
        title: s.title.clone(),
        subtitle: series_subtitle(s.is_subbed, s.is_dubbed),
        poster: pick(&s.images.poster_tall, POSTER_W),
        wide: pick(&s.images.poster_wide, WIDE_W),
        progress: 0.0,
        parent_id: None,
        badge: String::new(),
    }
}

pub fn movie_listing_card(m: &MovieListing) -> MediaCard {
    MediaCard {
        id: m.id.clone(),
        kind: MediaKind::Series,
        title: m.title.clone(),
        subtitle: "Filme".into(),
        poster: pick(&m.images.poster_tall, POSTER_W),
        wide: pick(&m.images.poster_wide, WIDE_W),
        progress: 0.0,
        parent_id: None,
        badge: String::new(),
    }
}

pub fn episode_card(e: &Episode, playhead: u32, fully_watched: bool) -> MediaCard {
    let label = format::episode_label(e.season_number, e.episode_number, &e.episode);
    let duration = e.duration.num_seconds().max(0) as u64;
    MediaCard {
        id: e.id.clone(),
        kind: MediaKind::Episode,
        title: e.series_title.clone(),
        subtitle: join_dot(&label, &e.title),
        poster: None,
        wide: pick(&e.images, WIDE_W),
        progress: format::progress(playhead, duration, fully_watched),
        parent_id: Some(e.series_id.clone()),
        badge: if e.is_premium_only { "PREMIUM".into() } else { String::new() },
    }
}

pub fn movie_card(m: &Movie, playhead: u32, fully_watched: bool) -> MediaCard {
    let duration = m.duration.num_seconds().max(0) as u64;
    MediaCard {
        id: m.id.clone(),
        kind: MediaKind::Movie,
        title: m.movie_listing_title.clone(),
        subtitle: m.title.clone(),
        poster: None,
        wide: pick(&m.images.thumbnail, WIDE_W),
        progress: format::progress(playhead, duration, fully_watched),
        parent_id: Some(m.movie_listing_id.clone()),
        badge: String::new(),
    }
}

/// Any collection item that makes sense as a card (seasons, music etc. are skipped).
pub fn collection_card(c: &MediaCollection, playhead: u32, fully_watched: bool) -> Option<MediaCard> {
    match c {
        MediaCollection::Series(s) => Some(series_card(s)),
        MediaCollection::MovieListing(m) => Some(movie_listing_card(m)),
        MediaCollection::Episode(e) => Some(episode_card(e, playhead, fully_watched)),
        MediaCollection::Movie(m) => Some(movie_card(m, playhead, fully_watched)),
        _ => None,
    }
}

fn series_subtitle(sub: bool, dub: bool) -> String {
    match (sub, dub) {
        (true, true) => "Legendado | Dublado".into(),
        (true, false) => "Legendado".into(),
        (false, true) => "Dublado".into(),
        _ => String::new(),
    }
}

fn join_dot(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (false, false) => format!("{a} · {b}"),
        (false, true) => a.to_string(),
        (true, _) => b.to_string(),
    }
}

/// Genre slug -> pt-BR label.
pub fn genre_name(slug: &str) -> String {
    match slug {
        "action" => "Ação",
        "adventure" => "Aventura",
        "comedy" => "Comédia",
        "drama" => "Drama",
        "fantasy" => "Fantasia",
        "music" => "Música",
        "romance" => "Romance",
        "sci-fi" => "Ficção científica",
        "seinen" => "Seinen",
        "shojo" => "Shoujo",
        "shonen" => "Shounen",
        "slice-of-life" => "Slice of life",
        "sports" => "Esportes",
        "supernatural" => "Sobrenatural",
        "thriller" => "Suspense",
        "horror" => "Terror",
        "mystery" => "Mistério",
        "isekai" => "Isekai",
        "historical" => "Histórico",
        "post-apocalyptic" => "Pós-apocalíptico",
        "idols" => "Idols",
        "anime" => "Anime",
        other => return other.replace('-', " "),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn img(w: u32, src: &str) -> serde_json::Value {
        json!({"source": src, "type": "x", "width": w, "height": w})
    }

    #[test]
    fn series_maps_to_poster_card() {
        let s: Series = serde_json::from_value(json!({
            "id": "GY8VEQ95Y", "title": "Frieren", "is_subbed": true, "is_dubbed": true,
            "images": {
                "poster_tall": [[img(240, "t240"), img(480, "t480")]],
                "poster_wide": [[img(640, "w640"), img(1920, "w1920")]]
            }
        }))
        .unwrap();
        let c = series_card(&s);
        assert_eq!(c.id, "GY8VEQ95Y");
        assert_eq!(c.kind, MediaKind::Series);
        assert_eq!(c.title, "Frieren");
        assert_eq!(c.subtitle, "Legendado | Dublado");
        assert_eq!(c.poster.as_deref(), Some("t480"));
        assert_eq!(c.wide.as_deref(), Some("w640"));
    }

    #[test]
    fn episode_maps_to_wide_card_with_progress() {
        let e: Episode = serde_json::from_value(json!({
            "id": "GRDKJZ81Y", "title": "O nome", "series_id": "GY8VEQ95Y", "series_title": "Frieren",
            "season_number": 2, "episode_number": 7, "episode": "7", "duration_ms": 1_420_000,
            "is_premium_only": true,
            "images": {"thumbnail": [[img(320, "th320"), img(640, "th640")]]}
        }))
        .unwrap();
        let c = episode_card(&e, 710, false);
        assert_eq!(c.kind, MediaKind::Episode);
        assert_eq!(c.title, "Frieren");
        assert_eq!(c.subtitle, "T2 E7 · O nome");
        assert_eq!(c.wide.as_deref(), Some("th640"));
        assert!((c.progress - 0.5).abs() < 0.001);
        assert_eq!(c.parent_id.as_deref(), Some("GY8VEQ95Y"));
        assert_eq!(c.badge, "PREMIUM");
    }

    #[test]
    fn genres_translate() {
        assert_eq!(genre_name("slice-of-life"), "Slice of life");
        assert_eq!(genre_name("action"), "Ação");
        assert_eq!(genre_name("new-genre"), "new genre");
    }
}
