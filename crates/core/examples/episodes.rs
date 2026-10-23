//! Dev tool: prints episode ids of a season (anonymous session).
use crunchyroll_rs::{Crunchyroll, Locale, Season};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let id = std::env::args().nth(1).unwrap_or_else(|| "GR19CPDWM".into());
    let cr = Crunchyroll::builder().locale(Locale::pt_BR).login_anonymously(Default::default()).await?;
    let s: Season = cr.media_from_id(&id).await?;
    for e in s.episodes().await?.iter().take(3) {
        println!("{} E{:?} {} premium={}", e.id, e.episode_number, e.title, e.is_premium_only);
    }
    Ok(())
}
