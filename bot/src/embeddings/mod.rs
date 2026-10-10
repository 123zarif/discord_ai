use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Service for generating local semantic vector embeddings using fastembed.
#[derive(Clone)]
pub struct EmbeddingEngine {
    model: Arc<Mutex<TextEmbedding>>,
}

impl EmbeddingEngine {
    /// Initializes the fastembed model (BAAI/bge-small-en-v1.5).
    pub fn new() -> anyhow::Result<Self> {
        let options = InitOptions::new(EmbeddingModel::BGESmallENV15)
            .with_show_download_progress(true);
        let model = TextEmbedding::try_new(options)?;

        Ok(Self {
            model: Arc::new(Mutex::new(model)),
        })
    }

    /// Generates 384-dimensional vector embeddings for a list of text strings.
    pub async fn embed_batch<S: AsRef<str> + Send>(&self, texts: &[S]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let documents: Vec<String> = texts.iter().map(|s| s.as_ref().to_string()).collect();
        let model_guard = self.model.clone();

        // Run embedding in a blocking thread to avoid blocking the Tokio async executor
        let embeddings = tokio::task::spawn_blocking(move || {
            let model = model_guard.blocking_lock();
            model.embed(documents, None)
        })
        .await??;

        Ok(embeddings)
    }
}
