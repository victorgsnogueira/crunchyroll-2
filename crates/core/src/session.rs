//! Login, saved session (refresh token in Windows Credential Manager) and profiles.

use crate::models::ProfileInfo;
use anyhow::{Context, Result};
use crunchyroll_rs::auth::{DeviceIdentifier, SessionToken};
use crunchyroll_rs::error::ErrorKind;
use crunchyroll_rs::{Crunchyroll, Locale};
use std::path::PathBuf;

const KEYRING_SERVICE: &str = "CrunchyrollDesktop";
const KEYRING_USER: &str = "refresh_token";

/// Error text shown on the login screen.
pub fn friendly_error(e: &crunchyroll_rs::Error) -> String {
    match e.kind() {
        ErrorKind::Authentication => "E-mail ou senha incorretos.".into(),
        ErrorKind::Block { .. } => {
            "A Crunchyroll bloqueou a conexão (proteção anti-robô). Tente de novo em alguns minutos.".into()
        }
        ErrorKind::Request { status: Some(s) } if s.as_u16() == 429 => {
            "Muitas tentativas. Aguarde um pouco e tente de novo.".into()
        }
        ErrorKind::Request { .. } => format!("Erro da Crunchyroll: {e}"),
        _ => format!("Não foi possível conectar: {e}"),
    }
}

/// Stores small, non-secret settings next to the cache.
pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "local", "CrunchyrollDesktop")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".data"))
}

/// A stable device id for this PC, created on first run.
pub fn device_id() -> String {
    let path = data_dir().join("device_id");
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(&path, &id);
    id
}

fn device() -> DeviceIdentifier {
    DeviceIdentifier {
        device_id: device_id(),
        device_type: "ANDROIDTV".into(),
        device_name: Some("Crunchyroll Desktop".into()),
    }
}

fn builder() -> crunchyroll_rs::auth::CrunchyrollBuilder {
    Crunchyroll::builder()
        .locale(Locale::pt_BR)
        .preferred_audio_locale(Locale::ja_JP)
}

pub struct Session {
    pub cr: Crunchyroll,
    pub account_id: String,
}

impl Session {
    pub async fn login(email: &str, password: &str) -> std::result::Result<Session, String> {
        let cr = builder()
            .login_with_credentials(email, password, device())
            .await
            .map_err(|e| friendly_error(&e))?;
        Self::finish(cr).await.map_err(|e| format!("{e:#}"))
    }

    /// Logs in with the saved refresh token, if there is one.
    pub async fn restore() -> Option<Session> {
        let token = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?.get_password().ok()?;
        let profile = std::fs::read_to_string(data_dir().join("profile_id")).ok();
        let result = match profile.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => builder().login_with_refresh_token_profile_id(token.as_str(), p, device()).await,
            None => builder().login_with_refresh_token(&token, device()).await,
        };
        match result {
            Ok(cr) => Self::finish(cr).await.ok(),
            Err(e) => {
                tracing::warn!("sessão salva inválida: {e}");
                if matches!(e.kind(), ErrorKind::Authentication) {
                    forget();
                }
                None
            }
        }
    }

    async fn finish(cr: Crunchyroll) -> Result<Session> {
        let account = cr.account().await.context("lendo a conta")?;
        let s = Session { cr, account_id: account.account_id };
        s.persist().await;
        Ok(s)
    }

    /// Saves the current refresh token (it rotates on refresh).
    pub async fn persist(&self) {
        if let SessionToken::RefreshToken(t) = self.cr.session_token().await {
            if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
                if let Err(e) = entry.set_password(&t) {
                    tracing::warn!("não salvou a sessão: {e}");
                }
            }
        }
    }

    pub async fn profiles(&self) -> Result<Vec<ProfileInfo>> {
        let p = self.cr.profiles().await?;
        Ok(p.profiles
            .into_iter()
            .map(|p| ProfileInfo {
                avatar_url: avatar_url(&p.avatar),
                id: p.profile_id,
                name: p.profile_name,
                selected: p.is_selected,
            })
            .collect())
    }

    /// New session bound to another profile of the same account.
    pub async fn switch_profile(&self, profile_id: &str) -> Result<Session> {
        let SessionToken::RefreshToken(token) = self.cr.session_token().await else {
            anyhow::bail!("sessão sem refresh token");
        };
        let cr = builder()
            .login_with_refresh_token_profile_id(token.as_str(), profile_id, device())
            .await
            .map_err(|e| anyhow::anyhow!(friendly_error(&e)))?;
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(data_dir().join("profile_id"), profile_id);
        Self::finish(cr).await
    }

    pub async fn access_token(&self) -> String {
        self.cr.access_token().await
    }
}

pub fn forget() {
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = e.delete_credential();
    }
    let _ = std::fs::remove_file(data_dir().join("profile_id"));
}

pub fn avatar_url(avatar: &str) -> String {
    if avatar.is_empty() {
        String::new()
    } else {
        format!("https://static.crunchyroll.com/assets/avatar/170x170/{avatar}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_urls() {
        assert_eq!(avatar_url("a.png"), "https://static.crunchyroll.com/assets/avatar/170x170/a.png");
        assert_eq!(avatar_url(""), "");
    }
}
