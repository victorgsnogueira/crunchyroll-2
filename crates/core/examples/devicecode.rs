//! Dev tool: checks whether the TV client can start a device-code login.
use crunchyroll_rs::auth::DevicePlatform;
use crunchyroll_rs::auth::platform_credentials::get_platform_credentials;
use crunchyroll_rs::Crunchyroll;

#[tokio::main]
async fn main() {
    let c = get_platform_credentials().await.expect("credenciais");
    let ua = c.android_tv_user_agent();
    let cr = Crunchyroll::builder()
        .platform(DevicePlatform::TvAndroid, c.android_tv.basic_auth_token.clone(), Some(ua.clone()))
        .login_anonymously(Default::default())
        .await
        .unwrap();
    for host in ["https://www.crunchyroll.com", "https://beta-api.crunchyroll.com"] {
        let r = cr
            .client()
            .post(format!("{host}/auth/v1/device/code"))
            .header("Authorization", format!("Basic {}", c.android_tv.basic_auth_token))
            .header("User-Agent", &ua)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("device_id=test-probe-0001&device_type=ANDROIDTV&device_name=Crunchyroll%20Desktop")
            .send()
            .await
            .unwrap();
        let status = r.status();
        let body = r.text().await.unwrap();
        let shown: String = body.chars().take(300).collect();
        let shown = regex_lite_mask(&shown);
        println!("{host}: HTTP {status}\n{shown}\n");
    }
}

fn regex_lite_mask(s: &str) -> String {
    match s.find("\"device_code\":\"") {
        Some(i) => {
            let start = i + 15;
            let end = s[start..].find('"').map(|e| start + e).unwrap_or(s.len());
            format!("{}<omitido>{}", &s[..start], &s[end..])
        }
        None => s.to_string(),
    }
}
