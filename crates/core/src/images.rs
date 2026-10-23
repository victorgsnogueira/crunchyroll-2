//! Image picking and a disk cache that returns decoded, resized RGBA pixels.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Minimal view of an API image (source + width), so this stays testable.
pub trait SizedImage {
    fn src(&self) -> &str;
    fn width(&self) -> u32;
}

impl SizedImage for crunchyroll_rs::common::Image {
    fn src(&self) -> &str {
        &self.source
    }
    fn width(&self) -> u32 {
        self.width
    }
}

/// Smallest image at least `target` px wide, else the widest one.
pub fn pick<I: SizedImage>(images: &[I], target: u32) -> Option<String> {
    let mut sorted: Vec<&I> = images.iter().filter(|i| !i.src().is_empty()).collect();
    sorted.sort_by_key(|i| i.width());
    sorted
        .iter()
        .find(|i| i.width() >= target)
        .or(sorted.last())
        .map(|i| i.src().to_string())
}

/// Home carousel images come as bare file names ("<uuid>.png"); the web app
/// serves them from static.crunchyroll.com/fms. Full URLs pass through.
pub fn fms_url(kind: FmsKind, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if name.starts_with("http://") || name.starts_with("https://") {
        return Some(name.to_string());
    }
    let id = name.rsplit_once('.').map(|(a, _)| a).unwrap_or(name);
    Some(match kind {
        FmsKind::Landscape => format!("https://static.crunchyroll.com/fms/landscape_large/1920x1080/94/png/{id}.webp"),
        FmsKind::Logo => format!("https://static.crunchyroll.com/fms/logo/85/png/{id}.webp"),
    })
}

#[derive(Clone, Copy, Debug)]
pub enum FmsKind {
    Landscape,
    Logo,
}

#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn cache_key(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub struct ImageCache {
    dir: PathBuf,
    http: reqwest::Client,
}

impl ImageCache {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).with_context(|| format!("criando cache {}", dir.display()))?;
        let http = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .build()?;
        Ok(Self { dir, http })
    }

    pub fn path_for(&self, url: &str) -> PathBuf {
        self.dir.join(cache_key(url))
    }

    /// Raw bytes from disk, or downloaded and stored.
    pub async fn bytes(&self, url: &str) -> Result<Vec<u8>> {
        let path = self.path_for(url);
        if let Ok(data) = tokio::fs::read(&path).await {
            return Ok(data);
        }
        let data = self
            .http
            .get(url)
            .header("Accept", "image/webp,image/jpeg,image/png,*/*")
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();
        let tmp = path.with_extension("part");
        if tokio::fs::write(&tmp, &data).await.is_ok() {
            let _ = tokio::fs::rename(&tmp, &path).await;
        }
        Ok(data)
    }

    /// Decoded pixels, downscaled so width <= `max_width` (0 = keep size).
    pub async fn load(&self, url: &str, max_width: u32) -> Result<DecodedImage> {
        let data = self.bytes(url).await?;
        tokio::task::spawn_blocking(move || decode(&data, max_width)).await?
    }
}

pub fn decode(data: &[u8], max_width: u32) -> Result<DecodedImage> {
    let img = image::load_from_memory(data).context("imagem inválida")?;
    let img = if max_width > 0 && img.width() > max_width {
        let h = (img.height() as u64 * max_width as u64 / img.width() as u64).max(1) as u32;
        img.resize_exact(max_width, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(DecodedImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

pub fn default_cache_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "local", "CrunchyrollDesktop")
        .map(|d| d.cache_dir().join("images"))
        .unwrap_or_else(|| Path::new(".cache/images").to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Img(&'static str, u32);
    impl SizedImage for Img {
        fn src(&self) -> &str {
            self.0
        }
        fn width(&self) -> u32 {
            self.1
        }
    }

    #[test]
    fn picks_smallest_that_fits_else_widest() {
        let imgs = [Img("a", 240), Img("b", 1920), Img("c", 480), Img("", 5000)];
        assert_eq!(pick(&imgs, 400).as_deref(), Some("c"));
        assert_eq!(pick(&imgs, 100).as_deref(), Some("a"));
        assert_eq!(pick(&imgs, 4000).as_deref(), Some("b"));
        assert_eq!(pick::<Img>(&[], 100), None);
    }

    #[test]
    fn fms_urls() {
        assert_eq!(
            fms_url(FmsKind::Landscape, "a5bac8ac-8bf6.png").as_deref(),
            Some("https://static.crunchyroll.com/fms/landscape_large/1920x1080/94/png/a5bac8ac-8bf6.webp")
        );
        assert_eq!(fms_url(FmsKind::Logo, "x.png").as_deref(), Some("https://static.crunchyroll.com/fms/logo/85/png/x.webp"));
        assert_eq!(fms_url(FmsKind::Logo, "https://a/b.png").as_deref(), Some("https://a/b.png"));
        assert_eq!(fms_url(FmsKind::Logo, " "), None);
    }

    #[test]
    fn cache_key_is_stable_hex() {
        let k = cache_key("https://x/y.jpg");
        assert_eq!(k.len(), 64);
        assert_eq!(k, cache_key("https://x/y.jpg"));
        assert_ne!(k, cache_key("https://x/z.jpg"));
    }

    #[test]
    fn decode_downscales_keeping_aspect() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(400, 200, image::Rgba([244, 117, 33, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let d = decode(&png, 100).unwrap();
        assert_eq!((d.width, d.height), (100, 50));
        assert_eq!(d.rgba.len(), 100 * 50 * 4);
        assert_eq!(&d.rgba[..4], &[244, 117, 33, 255]);
    }
}
