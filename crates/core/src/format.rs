//! Small text helpers for labels shown in the UI (pt-BR).

/// "23 min", "1 h 42 min", "45 s".
pub fn duration_label(total_secs: u64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    if h > 0 {
        if m > 0 { format!("{h} h {m} min") } else { format!("{h} h") }
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{total_secs} s")
    }
}

/// "8:41", "1:02:03".
pub fn clock(total_secs: u64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// "T2 E7". Falls back to the raw episode string when there is no number.
pub fn episode_label(season: u32, episode_number: Option<u32>, raw: &str) -> String {
    match episode_number {
        Some(n) if season > 0 => format!("T{season} E{n}"),
        Some(n) => format!("E{n}"),
        None if !raw.is_empty() => raw.to_string(),
        None => String::new(),
    }
}

/// Watched fraction clamped to 0..=1.
pub fn progress(playhead_secs: u32, duration_secs: u64, fully_watched: bool) -> f32 {
    if fully_watched {
        return 1.0;
    }
    if duration_secs == 0 {
        return 0.0;
    }
    (playhead_secs as f32 / duration_secs as f32).clamp(0.0, 1.0)
}

/// Where to resume: start over when (almost) finished.
pub fn resume_position(playhead_secs: u32, duration_secs: u64, fully_watched: bool) -> u32 {
    if fully_watched || (duration_secs > 0 && playhead_secs as u64 + 30 >= duration_secs) {
        0
    } else {
        playhead_secs
    }
}

/// Weekday name in pt-BR (0 = Monday).
pub fn weekday_pt(index: usize) -> &'static str {
    ["Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado", "Domingo"][index % 7]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration_label(23 * 60 + 40), "23 min");
        assert_eq!(duration_label(3600 + 42 * 60), "1 h 42 min");
        assert_eq!(duration_label(7200), "2 h");
        assert_eq!(duration_label(45), "45 s");
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(521), "8:41");
        assert_eq!(clock(3723), "1:02:03");
        assert_eq!(clock(0), "0:00");
    }

    #[test]
    fn episode_labels() {
        assert_eq!(episode_label(2, Some(7), "7"), "T2 E7");
        assert_eq!(episode_label(0, Some(3), "3"), "E3");
        assert_eq!(episode_label(1, None, "SP"), "SP");
    }

    #[test]
    fn progress_and_resume() {
        assert_eq!(progress(600, 1200, false), 0.5);
        assert_eq!(progress(5000, 1200, false), 1.0);
        assert_eq!(progress(10, 0, false), 0.0);
        assert_eq!(progress(0, 1200, true), 1.0);
        assert_eq!(resume_position(600, 1420, false), 600);
        assert_eq!(resume_position(1400, 1420, false), 0);
        assert_eq!(resume_position(600, 1420, true), 0);
    }
}
