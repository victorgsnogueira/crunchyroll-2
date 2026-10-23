//! The player page loads the DASH manifest through our local proxy, so relative
//! segment URLs would resolve against the proxy. We pin them to the CDN by adding
//! an MPD-level `<BaseURL>` that points at the manifest's original folder.

/// Folder of a URL, without query: "https://a/b/c.mpd?x=1" -> "https://a/b/".
pub fn url_folder(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    match no_query.rfind('/') {
        Some(i) => no_query[..=i].to_string(),
        None => no_query.to_string(),
    }
}

/// Adds `<BaseURL>{folder}</BaseURL>` as the first child of `<MPD>` unless the MPD
/// already has a top-level BaseURL. Returns the input unchanged if no `<MPD` tag exists.
pub fn pin_base_url(mpd: &str, manifest_url: &str) -> String {
    let Some(start) = mpd.find("<MPD") else {
        return mpd.to_string();
    };
    let Some(rel_end) = mpd[start..].find('>') else {
        return mpd.to_string();
    };
    let open_end = start + rel_end + 1;
    // Top-level BaseURL appears before the first <Period>.
    let first_period = mpd[open_end..].find("<Period").map(|i| open_end + i).unwrap_or(mpd.len());
    if mpd[open_end..first_period].contains("<BaseURL") {
        return mpd.to_string();
    }
    let folder = xml_escape(&url_folder(manifest_url));
    let mut out = String::with_capacity(mpd.len() + folder.len() + 24);
    out.push_str(&mpd[..open_end]);
    out.push_str("<BaseURL>");
    out.push_str(&folder);
    out.push_str("</BaseURL>");
    out.push_str(&mpd[open_end..]);
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_strips_file_and_query() {
        assert_eq!(url_folder("https://cdn.x/a/b/manifest.mpd?t=1&u=2"), "https://cdn.x/a/b/");
    }

    #[test]
    fn injects_base_url_after_mpd_open_tag() {
        let mpd = r#"<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011"><Period><AdaptationSet/></Period></MPD>"#;
        let out = pin_base_url(mpd, "https://cdn.x/v/manifest.mpd?a=1&b=2");
        assert!(out.contains(r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011"><BaseURL>https://cdn.x/v/</BaseURL><Period>"#));
    }

    #[test]
    fn keeps_existing_top_level_base_url() {
        let mpd = "<MPD><BaseURL>https://other/</BaseURL><Period/></MPD>";
        assert_eq!(pin_base_url(mpd, "https://cdn.x/v/m.mpd"), mpd);
    }

    #[test]
    fn representation_base_url_does_not_count_as_top_level() {
        let mpd = "<MPD><Period><Representation><BaseURL>seg/</BaseURL></Representation></Period></MPD>";
        let out = pin_base_url(mpd, "https://cdn.x/v/m.mpd");
        assert!(out.starts_with("<MPD><BaseURL>https://cdn.x/v/</BaseURL><Period>"));
    }

    #[test]
    fn escapes_ampersands_in_folder() {
        let out = pin_base_url("<MPD><Period/></MPD>", "https://cdn.x/a&b/m.mpd");
        assert!(out.contains("https://cdn.x/a&amp;b/"));
    }

    #[test]
    fn non_mpd_input_is_untouched() {
        assert_eq!(pin_base_url("{\"error\":1}", "https://x/m.mpd"), "{\"error\":1}");
    }
}
