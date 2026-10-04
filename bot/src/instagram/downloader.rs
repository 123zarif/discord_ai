use std::path::{Path, PathBuf};
use tracing::{info, warn};
use uuid::Uuid;

pub enum ReelDownload {
    DirectFile {
        path: PathBuf,
        size_mb: f64,
    },
    TooLarge {
        size_mb: f64,
        fallback_url: String,
    },
    Failed {
        reason: String,
        fallback_url: String,
    },
}

/// Converts a regular Instagram reel URL to a ddinstagram proxy URL that embeds in Discord natively.
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

/// Downloads an Instagram reel using yt-dlp.
pub async fn download_reel(reel_url: &str) -> ReelDownload {
    let fallback_url = to_ddinstagram_url(reel_url);
    let output_id = Uuid::new_v4();
    let output_path = PathBuf::from(format!("/tmp/reel_{output_id}.mp4"));

    let cookies = find_cookies_path();

    info!(
        reel_url,
        has_cookies = cookies.is_some(),
        "Downloading Instagram reel with yt-dlp..."
    );

    let mut cmd = tokio::process::Command::new("yt-dlp");

    if let Some(ref cookie_path) = cookies {
        cmd.arg("--cookies").arg(cookie_path);
    }

    cmd.arg("-o").arg(&output_path);
    cmd.arg("--no-playlist");
    cmd.arg(reel_url);

    let output = match cmd.output().await {
        Ok(out) => out,
        Err(e) => {
            warn!("Failed to execute yt-dlp command: {e}");
            return ReelDownload::Failed {
                reason: format!("yt-dlp execution error: {e}"),
                fallback_url,
            };
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("yt-dlp failed downloading reel: {stderr}");
        return ReelDownload::Failed {
            reason: stderr.chars().take(200).collect(),
            fallback_url,
        };
    }

    if !output_path.exists() {
        warn!("yt-dlp completed with success but output file does not exist");
        return ReelDownload::Failed {
            reason: "Download completed but file is missing".to_string(),
            fallback_url,
        };
    }

    // Check file size (Discord upload limit is 25MB for non-boosted bots)
    let metadata = match tokio::fs::metadata(&output_path).await {
        Ok(m) => m,
        Err(e) => {
            warn!("Could not read downloaded file metadata: {e}");
            let _ = tokio::fs::remove_file(&output_path).await;
            return ReelDownload::Failed {
                reason: "Could not read file size".to_string(),
                fallback_url,
            };
        }
    };

    let size_bytes = metadata.len();
    let size_mb = size_bytes as f64 / (1024.0 * 1024.0);

    if size_bytes > 25 * 1024 * 1024 {
        warn!(size_mb, "Reel exceeds 25MB Discord limit, falling back to proxy embed");
        let _ = tokio::fs::remove_file(&output_path).await;
        return ReelDownload::TooLarge {
            size_mb,
            fallback_url,
        };
    }

    ReelDownload::DirectFile {
        path: output_path,
        size_mb,
    }
}

/// Safely removes temporary file if it still exists.
pub async fn cleanup_file(path: &Path) {
    if path.exists() {
        let _ = tokio::fs::remove_file(path).await;
    }
}
