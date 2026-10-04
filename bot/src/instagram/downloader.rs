use std::path::PathBuf;
use tracing::{info, warn};
use uuid::Uuid;

pub enum MediaDownload {
    DirectFiles {
        paths: Vec<PathBuf>,
        total_size_mb: f64,
    },
    TooLarge {
        total_size_mb: f64,
        fallback_url: String,
    },
    Failed {
        reason: String,
        fallback_url: String,
    },
}

/// Converts a regular Instagram URL to an fxinstagram proxy URL that embeds in Discord natively.
pub fn to_fxinstagram_url(url: &str) -> String {
    if url.contains("www.instagram.com") {
        url.replace("www.instagram.com", "fxinstagram.com")
    } else if url.contains("instagram.com") {
        url.replace("instagram.com", "fxinstagram.com")
    } else if url.contains("instagr.am") {
        url.replace("instagr.am", "fxinstagram.com")
    } else {
        url.to_string()
    }
}

/// Checks if a cookies.txt file exists either in /app or current directory.
pub fn find_cookies_path() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("/app/cookies.txt"),
        PathBuf::from("cookies.txt"),
        PathBuf::from("./bot/cookies.txt"),
    ];

    for path in &candidates {
        if path.exists() {
            return Some(path.clone());
        }
    }
    None
}

/// Extracts an image URL from HTML metadata tags (og:image or twitter:image).
fn extract_meta_image(html: &str) -> Option<String> {
    for pattern in &["property=\"og:image\" content=\"", "name=\"twitter:image\" content=\""] {
        if let Some(start_idx) = html.find(pattern) {
            let rest = &html[start_idx + pattern.len()..];
            if let Some(end_idx) = rest.find('"') {
                let raw_url = &rest[..end_idx];
                let decoded = raw_url.replace("&amp;", "&");
                if decoded.starts_with("http") {
                    return Some(decoded);
                }
            }
        }
    }
    None
}

/// Inspects the header magic bytes of a file buffer to detect the actual format.
fn detect_media_extension(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[0..3] == [0xFF, 0xD8, 0xFF] {
        "jpg"
    } else if bytes.len() >= 8 && bytes[0..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        "png"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.len() >= 3 && &bytes[0..3] == b"GIF" {
        "gif"
    } else if bytes.len() >= 8 && (&bytes[4..8] == b"ftyp" || &bytes[4..8] == b"moov") {
        "mp4"
    } else {
        "bin"
    }
}

/// Directly downloads a CDN asset (image or video) via HTTP with magic byte validation.
async fn download_direct_cdn(url: &str, file_prefix: &str) -> Option<PathBuf> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let bytes = resp.bytes().await.ok()?;
    if bytes.len() < 512 {
        return None;
    }

    let ext = detect_media_extension(&bytes);
    if ext == "bin" {
        warn!("Direct CDN asset has unrecognized media header");
        return None;
    }

    let dir = format!("/tmp/{file_prefix}");
    tokio::fs::create_dir_all(&dir).await.ok()?;

    let filename = if ext == "mp4" {
        "video.mp4"
    } else {
        match ext {
            "jpg" => "photo.jpg",
            "png" => "photo.png",
            "webp" => "photo.webp",
            "gif" => "animation.gif",
            _ => "media.bin",
        }
    };

    let target_path = PathBuf::from(format!("{dir}/{filename}"));
    tokio::fs::write(&target_path, &bytes).await.ok()?;
    Some(target_path)
}

/// Downloads Instagram media (reel, post, or collection) using yt-dlp,
/// falling back to fxinstagram direct image extraction for photo carousels/posts.
pub async fn download_media(media_url: &str) -> MediaDownload {
    let fallback_url = to_fxinstagram_url(media_url);
    let output_id = Uuid::new_v4();
    let file_prefix = format!("ig_{output_id}");
    let dir = format!("/tmp/{file_prefix}");
    let _ = tokio::fs::create_dir_all(&dir).await;

    // If this is a direct CDN asset link (e.g. from Meta's lookaside / cdn servers)
    if media_url.contains("fbsbx.com")
        || media_url.contains("cdninstagram.com")
        || media_url.contains("fbcdn.net")
    {
        info!(media_url, "Downloading direct Instagram CDN media...");
        if let Some(path) = download_direct_cdn(media_url, &file_prefix).await {
            if let Ok(meta) = tokio::fs::metadata(&path).await {
                let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
                if size_mb <= 20.0 {
                    return MediaDownload::DirectFiles {
                        paths: vec![path],
                        total_size_mb: size_mb,
                    };
                } else {
                    let _ = tokio::fs::remove_file(&path).await;
                    return MediaDownload::TooLarge {
                        total_size_mb: size_mb,
                        fallback_url,
                    };
                }
            }
        }
    }

    let output_template = format!("{dir}/reel_%(autonumber)02d.%(ext)s");
    let cookies = find_cookies_path();

    info!(
        media_url,
        has_cookies = cookies.is_some(),
        "Downloading Instagram media with yt-dlp..."
    );

    let mut cmd = tokio::process::Command::new("yt-dlp");

    if let Some(ref cookie_path) = cookies {
        cmd.arg("--cookies").arg(cookie_path);
    }

    cmd.arg("-o").arg(&output_template);
    cmd.arg("--max-downloads").arg("10");
    cmd.arg(media_url);

    let output = match cmd.output().await {
        Ok(out) => out,
        Err(e) => {
            warn!("Failed to execute yt-dlp command: {e}");
            return MediaDownload::Failed {
                reason: format!("yt-dlp execution error: {e}"),
                fallback_url,
            };
        }
    };

    // Scan temporary directory for files
    let mut downloaded_files = Vec::new();
    let mut total_size_bytes = 0u64;

    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                total_size_bytes += meta.len();
                downloaded_files.push(entry.path());
            }
        }
    }

    downloaded_files.sort();

    // If yt-dlp produced no files (e.g. photo posts or carousels where yt-dlp found "No video formats found!")
    if downloaded_files.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        info!("yt-dlp produced no video files for media, attempting image extraction from fxinstagram...");

        let fx_url = to_fxinstagram_url(media_url);
        info!(fx_url = %fx_url, "Querying fxinstagram for post image metadata");

        let client = reqwest::Client::builder()
            .user_agent("Discordbot/2.0")
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        match client.get(&fx_url).send().await {
            Ok(resp) => {
                info!(status = %resp.status(), "Received response from fxinstagram");
                match resp.text().await {
                    Ok(html) => {
                        if let Some(img_url) = extract_meta_image(&html) {
                            info!(img_url = %img_url, "Found image in fxinstagram metadata, downloading photo...");
                            match client.get(&img_url).send().await {
                                Ok(img_resp) => {
                                    if img_resp.status().is_success() {
                                        if let Ok(bytes) = img_resp.bytes().await {
                                            let photo_path = PathBuf::from(format!("{dir}/photo.jpg"));
                                            if let Err(e) = tokio::fs::write(&photo_path, &bytes).await {
                                                warn!("Failed to write photo to disk: {e}");
                                            } else {
                                                let size_mb = bytes.len() as f64 / (1024.0 * 1024.0);
                                                info!(size_mb, "Successfully downloaded photo for post");
                                                return MediaDownload::DirectFiles {
                                                    paths: vec![photo_path],
                                                    total_size_mb: size_mb,
                                                };
                                            }
                                        }
                                    } else {
                                        warn!(status = %img_resp.status(), "Failed to download image from CDN");
                                    }
                                }
                                Err(e) => warn!("Failed to fetch image bytes from CDN: {e}"),
                            }
                        } else {
                            warn!("No image metadata tag found in fxinstagram response");
                        }
                    }
                    Err(e) => warn!("Failed to read fxinstagram response text: {e}"),
                }
            }
            Err(e) => warn!("Failed to query fxinstagram at {fx_url}: {e}"),
        }

        warn!("yt-dlp produced no files and photo extraction could not resolve: {stderr}");
        return MediaDownload::Failed {
            reason: stderr.chars().take(200).collect(),
            fallback_url,
        };
    }

    let total_size_mb = total_size_bytes as f64 / (1024.0 * 1024.0);

    // Check Discord file limit (20MB total per message)
    if total_size_bytes > 20 * 1024 * 1024 {
        warn!(
            total_size_mb,
            "Downloaded media exceeds 20MB Discord limit, falling back to proxy embed"
        );
        cleanup_files(&downloaded_files).await;
        return MediaDownload::TooLarge {
            total_size_mb,
            fallback_url,
        };
    }

    MediaDownload::DirectFiles {
        paths: downloaded_files,
        total_size_mb,
    }
}

/// Safely removes temporary files and folders if they still exist.
pub async fn cleanup_files(paths: &[PathBuf]) {
    for p in paths {
        if p.exists() {
            let _ = tokio::fs::remove_file(p).await;
            if let Some(parent) = p.parent() {
                let _ = tokio::fs::remove_dir_all(parent).await;
            }
        }
    }
}
