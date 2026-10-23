//! Dev tool: prints the raw season list of a series (anonymous session).
//! cargo run -p cr-core --example seasons -- GDKHZEJ0K
use crunchyroll_rs::{Crunchyroll, Locale, Series};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let id = std::env::args().nth(1).unwrap_or_else(|| "GDKHZEJ0K".into());
    let cr = Crunchyroll::builder()
        .locale(Locale::pt_BR)
        .preferred_audio_locale(Locale::ja_JP)
        .login_anonymously(Default::default())
        .await?;
    let s: Series = cr.media_from_id(&id).await?;
    println!("{} audio={:?}", s.title, s.audio_locales);
    for season in s.seasons().await? {
        println!(
            "season {} n={} title={:?} audio={:?} versions={:?}",
            season.id,
            season.season_number,
            season.title,
            season.audio_locales,
            season.versions.iter().map(|v| (v.id.clone(), v.audio_locale.to_string(), v.original)).collect::<Vec<_>>()
        );
    }
    Ok(())
}
