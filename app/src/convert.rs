//! cr-core models -> Slint structs (images start empty and stream in later).

use crate::{CalendarDayData, CalendarItemData, CardData, DetailData, EpisodeData, HeroData, ListData, ProfileData, SeasonData};
use cr_core::models::*;
use slint::{Image, ModelRc, SharedString, VecModel};
use std::rc::Rc;

pub fn card(c: &MediaCard) -> CardData {
    CardData {
        id: c.id.clone().into(),
        kind: match c.kind {
            MediaKind::Series => 0,
            MediaKind::Movie => 1,
            MediaKind::Episode => 2,
        },
        title: c.title.clone().into(),
        subtitle: c.subtitle.clone().into(),
        image: Image::default(),
        has_image: false,
        progress: c.progress,
        badge: c.badge.clone().into(),
        parent_id: c.parent_id.clone().unwrap_or_default().into(),
    }
}

/// Which image a card shows: posters for poster rows, wide art otherwise.
pub fn card_image(c: &MediaCard, wide: bool) -> Option<String> {
    if wide { c.wide.clone().or_else(|| c.poster.clone()) } else { c.poster.clone().or_else(|| c.wide.clone()) }
}

pub fn hero(h: &Hero) -> HeroData {
    HeroData {
        series_id: h.series_id.clone().into(),
        title: h.title.clone().into(),
        description: h.description.clone().into(),
        image: Image::default(),
        has_image: false,
        logo: Image::default(),
        has_logo: false,
    }
}

pub fn detail(d: &SeriesDetail) -> DetailData {
    DetailData {
        id: d.id.clone().into(),
        title: d.title.clone().into(),
        description: d.description.clone().into(),
        image: Image::default(),
        has_image: false,
        poster: Image::default(),
        has_poster: false,
        meta: d.meta_line.clone().into(),
        genres: d.genres.join(" · ").into(),
        audio: d.audio.join(", ").into(),
        subtitles: d.subtitles.join(", ").into(),
        maturity: d.maturity.clone().into(),
        in_watchlist: d.in_watchlist,
        rating: d.rating as f32,
        rating_count: d.rating_count as i32,
        is_movie: d.kind == MediaKind::Movie,
        play_label: "ASSISTIR".into(),
        play_id: SharedString::default(),
    }
}

pub fn season(s: &SeasonInfo) -> SeasonData {
    SeasonData { id: s.id.clone().into(), label: s.label.clone().into() }
}

pub fn episode(e: &EpisodeItem) -> EpisodeData {
    EpisodeData {
        id: e.id.clone().into(),
        label: e.label.clone().into(),
        title: e.title.clone().into(),
        description: e.description.clone().into(),
        duration: e.duration_label.clone().into(),
        thumb: Image::default(),
        has_thumb: false,
        progress: e.progress,
        premium: e.premium_only,
    }
}

pub fn profile(p: &ProfileInfo) -> ProfileData {
    ProfileData {
        id: p.id.clone().into(),
        name: p.name.clone().into(),
        avatar: Image::default(),
        has_avatar: false,
        selected: p.selected,
    }
}

pub fn list(l: &ListInfo) -> ListData {
    ListData { id: l.id.clone().into(), title: l.title.clone().into(), count: l.count as i32 }
}

pub fn calendar_day(d: &CalendarDay, today: bool) -> (CalendarDayData, Rc<VecModel<CalendarItemData>>) {
    let items = Rc::new(VecModel::from(
        d.items
            .iter()
            .map(|i| CalendarItemData {
                series_id: i.series_id.clone().into(),
                episode_id: i.episode_id.clone().into(),
                title: i.title.clone().into(),
                episode: i.episode_label.clone().into(),
                time: i.time_label.clone().into(),
                thumb: Image::default(),
                has_thumb: false,
                premium: i.premium,
            })
            .collect::<Vec<_>>(),
    ));
    (CalendarDayData { label: d.label.clone().into(), today, items: ModelRc::from(items.clone()) }, items)
}
