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

/// Converts a regular Instagram URL to a ddinstagram proxy URL that embeds in Discord natively.
pub fn to_ddinstagram_url(url: &str) -> String {
    if url.contains("instagram.com") {
        url.replace("instagram.com", "ddinstagram.com")
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

/// Downloads Instagram media (reel, post, or collection) using yt-dlp.
pub async fn download_media(media_url: &str) -> MediaDownload {
    let fallback_url = to_ddinstagram_url(media_url);
    let output_id = Uuid::new_v4();
    let file_prefix = format!("ig_{output_id}");
    let output_template = format!("/tmp/{file_prefix}_%(autonumber)02d.%(ext)s");

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

    // Scan /tmp for files matching our unique prefix
    let mut downloaded_files = Vec::new();
    let mut total_size_bytes = 0u64;

    if let Ok(mut entries) = tokio::fs::read_dir("/tmp").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with(&file_prefix) {
                if let Ok(meta) = entry.metadata().await {
                    total_size_bytes += meta.len();
                    downloaded_files.push(entry.path());
                }
            }
        }
    }

    downloaded_files.sort();

    if downloaded_files.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("yt-dlp produced no files for media: {stderr}");
        return MediaDownload::Failed {
            reason: stderr.chars().take(200).collect(),
            fallback_url,
        };
    }

    let total_size_mb = total_size_bytes as f64 / (1024.0 * 1024.0);

    // Check Discord file limit (25MB total per message)
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

/// Safely removes temporary files if they still exist.
pub async fn cleanup_files(paths: &[PathBuf]) {
    for p in paths {
        if p.exists() {
            let _ = tokio::fs::remove_file(p).await;
        }
    }
}
