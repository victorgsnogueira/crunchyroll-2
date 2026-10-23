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

/// The official apps' client credentials rotate; the copy bundled in
/// crunchyroll-rs goes stale. We fetch the current ones from
/// crunchy-labs/artifacts and keep a local copy (refreshed daily).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ClientCredentials {
    pub client_id: String,
    pub basic_auth_token: String,
    pub user_agent: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct AppCredentials {
    tv: ClientCredentials,
    phone: ClientCredentials,
    fetched_at: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

async fn app_credentials(force_refresh: bool) -> Option<AppCredentials> {
    let path = data_dir().join("app_credentials.json");
    let cached: Option<AppCredentials> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(c) = &cached {
        if !force_refresh && now_secs().saturating_sub(c.fetched_at) < 24 * 3600 {
            return cached;
        }
    }
    match crunchyroll_rs::auth::platform_credentials::get_platform_credentials().await {
        Ok(c) => {
            let creds = AppCredentials {
                tv: ClientCredentials {
                    client_id: c.android_tv.client_id.clone(),
                    basic_auth_token: c.android_tv.basic_auth_token.clone(),
                    user_agent: c.android_tv_user_agent(),
                },
                phone: ClientCredentials {
                    client_id: c.android_phone.client_id.clone(),
                    basic_auth_token: c.android_phone.basic_auth_token.clone(),
                    user_agent: c.android_phone_user_agent(),
                },
                fetched_at: now_secs(),
            };
            let _ = std::fs::create_dir_all(data_dir());
            let _ = std::fs::write(&path, serde_json::to_vec(&creds).unwrap_or_default());
            Some(creds)
        }
        Err(e) => {
            tracing::warn!("não buscou credenciais atuais dos apps: {e}");
            cached
        }
    }
}

/// Which official app identity a session belongs to. Refresh tokens only work
/// with the client that issued them, so this is saved with the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Android TV client: code login (crunchyroll.com/activate).
    Tv,
    /// Android phone client: official SSO page login.
    Phone,
}

fn saved_platform() -> Platform {
    match std::fs::read_to_string(data_dir().join("platform")).as_deref().map(str::trim) {
        Ok("phone") => Platform::Phone,
        _ => Platform::Tv,
    }
}

fn save_platform(p: Platform) {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(data_dir().join("platform"), if p == Platform::Phone { "phone" } else { "tv" });
}

fn device_for(p: Platform) -> DeviceIdentifier {
    DeviceIdentifier {
        device_id: device_id(),
        device_type: match p {
            Platform::Tv => "ANDROIDTV".into(),
            Platform::Phone => "Windows PC".into(),
        },
        device_name: Some("Crunchyroll Desktop".into()),
    }
}

fn device() -> DeviceIdentifier {
    device_for(Platform::Tv)
}

async fn client_for(p: Platform) -> Option<ClientCredentials> {
    let c = app_credentials(false).await?;
    Some(match p {
        Platform::Tv => c.tv,
        Platform::Phone => c.phone,
    })
}

/// Builder with the current credentials of `p` (falls back to the bundled TV ones).
async fn builder_for(p: Platform) -> crunchyroll_rs::auth::CrunchyrollBuilder {
    let b = builder();
    let dp = match p {
        Platform::Tv => crunchyroll_rs::auth::DevicePlatform::TvAndroid,
        Platform::Phone => crunchyroll_rs::auth::DevicePlatform::AndroidPhone,
    };
    match client_for(p).await {
        Some(c) => b.platform(dp, c.basic_auth_token, Some(c.user_agent)),
        None => b,
    }
}

fn builder() -> crunchyroll_rs::auth::CrunchyrollBuilder {
    Crunchyroll::builder()
        .locale(Locale::pt_BR)
        .preferred_audio_locale(Locale::ja_JP)
}

/// PKCE verifier for the SSO login (43+ chars from the unreserved set).
pub fn new_code_verifier() -> String {
    let a = uuid::Uuid::new_v4().simple().to_string();
    let b = uuid::Uuid::new_v4().simple().to_string();
    format!("{a}{b}")
}

/// Client id of the phone app, used to open the SSO page.
pub async fn sso_client_id() -> Result<String> {
    Ok(client_for(Platform::Phone).await.context("credenciais do app de celular indisponíveis")?.client_id)
}

/// TV-style sign-in: the user approves `user_code` at crunchyroll.com/activate.
#[derive(Clone, Debug)]
pub struct DeviceLogin {
    pub user_code: String,
    device_code: String,
    pub interval: std::time::Duration,
    pub expires_at: std::time::Instant,
}

pub const ACTIVATE_URL: &str = "https://www.crunchyroll.com/activate";

/// Crunchyroll reports the polling interval in ms (sometimes seconds); keep it sane.
pub fn normalize_interval_ms(raw: u64) -> u64 {
    let ms = if raw > 100 { raw } else if raw == 0 { 5000 } else { raw * 1000 };
    ms.clamp(1000, 5000)
}

async fn auth_http() -> Result<(reqwest::Client, ClientCredentials)> {
    let creds = client_for(Platform::Tv).await.context("credenciais do app de TV indisponíveis")?;
    let http = crunchyroll_rs::auth::CrunchyrollBuilder::predefined_client_builder()
        .user_agent(creds.user_agent.clone())
        .build()?;
    Ok((http, creds))
}

pub enum DevicePoll {
    Pending,
    Done(Session),
    Expired,
}

pub struct Session {
    pub cr: Crunchyroll,
    pub account_id: String,
    pub email: String,
}

impl Session {
    /// Finishes the official SSO page login: exchanges the redirect code.
    pub async fn login_with_sso_code(code: &str, verifier: &str) -> std::result::Result<Session, String> {
        let result = builder_for(Platform::Phone)
            .await
            .login_with_oauth_code(code, verifier, device_for(Platform::Phone))
            .await;
        match result {
            Ok(cr) => {
                tracing::info!("login pelo SSO ok");
                save_platform(Platform::Phone);
                Self::finish(cr).await.map_err(|e| format!("{e:#}"))
            }
            Err(e) => {
                tracing::warn!("troca do código do SSO falhou: {:?} {e}", e.kind());
                Err(friendly_error(&e))
            }
        }
    }

    /// Starts a TV-style login and returns the code to show the user.
    pub async fn start_device_login() -> Result<DeviceLogin> {
        #[derive(serde::Deserialize)]
        struct Resp {
            user_code: String,
            device_code: String,
            #[serde(default)]
            interval: u64,
            #[serde(default)]
            expires_in: u64,
        }
        let (http, creds) = auth_http().await?;
        let body = format!(
            "device_id={}&device_type=ANDROIDTV&device_name=Crunchyroll%20Desktop",
            device_id()
        );
        let resp = http
            .post("https://www.crunchyroll.com/auth/v1/device/code")
            .header("Authorization", format!("Basic {}", creds.basic_auth_token))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("a Crunchyroll recusou gerar o código ({status})");
        }
        let r: Resp = serde_json::from_str(&text).context("resposta inesperada ao gerar o código")?;
        Ok(DeviceLogin {
            user_code: r.user_code.to_uppercase(),
            device_code: r.device_code,
            interval: std::time::Duration::from_millis(normalize_interval_ms(r.interval)),
            expires_at: std::time::Instant::now()
                + std::time::Duration::from_secs(if r.expires_in == 0 { 300 } else { r.expires_in }),
        })
    }

    /// Checks once whether the code was approved.
    pub async fn poll_device_login(d: &DeviceLogin) -> Result<DevicePoll> {
        if std::time::Instant::now() >= d.expires_at {
            return Ok(DevicePoll::Expired);
        }
        let (http, creds) = auth_http().await?;
        let resp = http
            .post("https://www.crunchyroll.com/auth/v1/device/token")
            .header("Authorization", format!("Basic {}", creds.basic_auth_token))
            .json(&serde_json::json!({ "device_id": device_id(), "device_code": d.device_code }))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.is_success() {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if let Some(refresh) = v.get("refresh_token").and_then(|t| t.as_str()) {
                let cr = builder_for(Platform::Tv)
                    .await
                    .login_with_refresh_token(refresh, device())
                    .await
                    .map_err(|e| anyhow::anyhow!(friendly_error(&e)))?;
                tracing::info!("login por código ok");
                save_platform(Platform::Tv);
                return Ok(DevicePoll::Done(Self::finish(cr).await?));
            }
            return Ok(DevicePoll::Pending);
        }
        if text.to_lowercase().contains("expired") {
            return Ok(DevicePoll::Expired);
        }
        Ok(DevicePoll::Pending)
    }

    /// Logs in with the saved refresh token, if there is one.
    pub async fn restore() -> Option<Session> {
        let token = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?.get_password().ok()?;
        let profile = std::fs::read_to_string(data_dir().join("profile_id")).ok();
        let result = match profile.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => {
                builder_for(saved_platform())
                    .await
                    .login_with_refresh_token_profile_id(token.as_str(), p, device_for(saved_platform()))
                    .await
            }
            None => builder_for(saved_platform()).await.login_with_refresh_token(&token, device_for(saved_platform())).await,
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

    /// Development only: browse the catalog without an account (no playback/lists).
    pub async fn anonymous() -> std::result::Result<Session, String> {
        let cr = builder().login_anonymously(device()).await.map_err(|e| friendly_error(&e))?;
        Ok(Session { cr, account_id: String::new(), email: "visitante (modo de teste)".into() })
    }

    async fn finish(cr: Crunchyroll) -> Result<Session> {
        let account = cr.account().await.context("lendo a conta")?;
        let s = Session { cr, account_id: account.account_id, email: account.email };
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
        let cr = builder_for(saved_platform())
            .await
            .login_with_refresh_token_profile_id(token.as_str(), profile_id, device_for(saved_platform()))
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
    fn device_interval_is_normalized() {
        assert_eq!(normalize_interval_ms(500), 1000);
        assert_eq!(normalize_interval_ms(3000), 3000);
        assert_eq!(normalize_interval_ms(5), 5000);
        assert_eq!(normalize_interval_ms(2), 2000);
        assert_eq!(normalize_interval_ms(0), 5000);
    }

    #[test]
    fn code_verifier_is_valid_pkce() {
        let v = new_code_verifier();
        assert!(v.len() >= 43 && v.len() <= 128);
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(v, new_code_verifier());
    }

    #[test]
    fn avatar_urls() {
        assert_eq!(avatar_url("a.png"), "https://static.crunchyroll.com/assets/avatar/170x170/a.png");
        assert_eq!(avatar_url(""), "");
    }
}
