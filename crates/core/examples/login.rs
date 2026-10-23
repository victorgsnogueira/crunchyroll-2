//! Dev tool: checks that the current Android TV client credentials are accepted,
//! using a fake account (expects "invalid_credentials", not a client error).
use crunchyroll_rs::auth::DevicePlatform;
use crunchyroll_rs::auth::platform_credentials::get_platform_credentials;
use crunchyroll_rs::{Crunchyroll, Locale};

#[tokio::main]
async fn main() {
    let c = get_platform_credentials().await.expect("credenciais");
    let anon = Crunchyroll::builder()
        .platform(DevicePlatform::TvAndroid, c.android_tv.basic_auth_token.clone(), Some(c.android_tv_user_agent()))
        .login_anonymously(Default::default())
        .await;
    println!("anônimo com credencial atual: {}", if anon.is_ok() { "OK" } else { "FALHOU" });
    let r = Crunchyroll::builder()
        .locale(Locale::pt_BR)
        .platform(DevicePlatform::TvAndroid, c.android_tv.basic_auth_token.clone(), Some(c.android_tv_user_agent()))
        .login_with_credentials("nobody@example.com", "wrong-password-123", Default::default())
        .await;
    match r {
        Ok(_) => println!("LOGIN OK"),
        Err(e) => println!("conta falsa: kind={:?} msg={e}", e.kind()),
    }
}
