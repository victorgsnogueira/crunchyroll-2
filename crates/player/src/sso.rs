//! Official Crunchyroll sign-in page (sso.crunchyroll.com) in a WebView2 panel,
//! the same flow the official Android app uses. The user types e-mail and
//! password on Crunchyroll's own page; we only catch the final
//! `sso.crunchyroll://auth?code=...` redirect.

use crate::{Bounds, query_param};
use anyhow::{Result, anyhow};
use raw_window_handle::HasWindowHandle;
use std::path::PathBuf;
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder};

#[cfg(windows)]
use wry::WebViewBuilderExtWindows;

pub const REDIRECT_URI: &str = "sso.crunchyroll://auth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsoResult {
    Code(String),
    Error(String),
}

/// Authorize URL (PKCE "plain": the verifier is sent as the challenge).
pub fn authorize_url(client_id: &str, verifier: &str) -> String {
    format!(
        "https://sso.crunchyroll.com/authorize?client_id={client_id}&redirect_uri={}&scope=offline_access&code_challenge={verifier}&code_challenge_method=plain&response_type=code",
        REDIRECT_URI
    )
}

/// Recognizes the final redirect. Returns None for normal page navigations.
pub fn parse_redirect(url: &str) -> Option<SsoResult> {
    let rest = url.strip_prefix(REDIRECT_URI)?;
    let query = rest.split_once('?').map(|(_, q)| q).unwrap_or("");
    let query = query.split('#').next().unwrap_or("");
    if let Some(code) = query_param(query, "code").filter(|c| !c.is_empty()) {
        return Some(SsoResult::Code(code));
    }
    let err = query_param(query, "error_description")
        .or_else(|| query_param(query, "error"))
        .unwrap_or_else(|| "login cancelado".into());
    Some(SsoResult::Error(err))
}

pub struct SsoView {
    webview: WebView,
    _context: WebContext,
}

impl SsoView {
    pub fn new<W: HasWindowHandle>(
        parent: &W,
        bounds: Bounds,
        data_dir: PathBuf,
        url: &str,
        on_result: impl Fn(SsoResult) + 'static,
    ) -> Result<Self> {
        let mut context = WebContext::new(Some(data_dir));
        let builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_bounds((&bounds).into())
            .with_background_color((0, 0, 0, 255))
            .with_devtools(cfg!(debug_assertions))
            .with_navigation_handler(move |url: String| match parse_redirect(&url) {
                Some(r) => {
                    on_result(r);
                    false
                }
                None => true,
            })
            // "Esqueceu a senha?", termos etc. open in the normal browser
            .with_new_window_req_handler(|url, _features| {
                if url.starts_with("https://") {
                    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn();
                }
                NewWindowResponse::Deny
            })
            .with_url(url);
        #[cfg(windows)]
        let builder = builder.with_browser_accelerator_keys(false).with_default_context_menus(false);
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

    pub fn load(&self, url: &str) {
        let _ = self.webview.load_url(url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_authorize_url() {
        let u = authorize_url("abc", "VERIFIER");
        assert!(u.starts_with("https://sso.crunchyroll.com/authorize?client_id=abc&"));
        assert!(u.contains("redirect_uri=sso.crunchyroll://auth"));
        assert!(u.contains("code_challenge=VERIFIER&code_challenge_method=plain"));
        assert!(u.contains("response_type=code"));
    }

    #[test]
    fn parses_redirects() {
        assert_eq!(parse_redirect("sso.crunchyroll://auth?code=XyZ123&state=1"), Some(SsoResult::Code("XyZ123".into())));
        assert_eq!(parse_redirect("sso.crunchyroll://auth/?code=a%2Bb"), Some(SsoResult::Code("a+b".into())));
        assert_eq!(
            parse_redirect("sso.crunchyroll://auth?error=access_denied&error_description=Nope"),
            Some(SsoResult::Error("Nope".into()))
        );
        assert_eq!(parse_redirect("https://sso.crunchyroll.com/pt-br/login"), None);
    }
}
