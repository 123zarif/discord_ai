use reqwest::Client;
use serde::{Deserialize, Serialize};

pub const CINEMETA_BASE_URL: &str = "https://v3-cinemeta.strem.io";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CinemetaMedia {
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(rename = "type")]
    pub media_type: String,
    pub name: String,
    pub year: Option<String>,
    pub released: Option<String>,
    pub poster: Option<String>,
    pub background: Option<String>,
    #[serde(default, rename = "genre")]
    pub genres: Vec<String>,
    #[serde(rename = "imdbRating")]
    pub imdb_rating: Option<String>,
    pub runtime: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
}

impl CinemetaMedia {
    /// Safe identifier (IMDb ID or slug).
    pub fn id(&self) -> &str {
        self.imdb_id.as_deref().unwrap_or("unknown")
    }

    /// Clean synopsis description.
    pub fn clean_description(&self) -> &str {
        match &self.description {
            Some(desc) if !desc.trim().is_empty() => desc.trim(),
            _ => "No synopsis available.",
        }
    }

    /// Best available release year string.
    pub fn display_year(&self) -> Option<&str> {
        if let Some(ref y) = self.year {
            if !y.trim().is_empty() {
                return Some(y.as_str());
            }
        }
        if let Some(ref r) = self.released {
            if r.len() >= 4 {
                return Some(&r[..4]);
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CinemetaSearchItem {
    pub id: String,
    #[serde(rename = "type")]
    pub media_type: String,
    pub name: String,
    pub poster: Option<String>,
    #[serde(rename = "releaseInfo")]
    pub release_info: Option<String>,
    pub year: Option<String>,
}

impl CinemetaSearchItem {
    pub fn display_label(&self) -> String {
        let yr = self
            .release_info
            .as_deref()
            .or(self.year.as_deref())
            .unwrap_or("");

        let label = if yr.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, yr)
        };

        label.chars().take(95).collect()
    }
}

#[derive(Deserialize)]
struct CatalogResponse {
    #[serde(default)]
    metas: Vec<CinemetaSearchItem>,
}

#[derive(Deserialize)]
struct MetaResponse {
    meta: Option<CinemetaMedia>,
}

/// Searches Cinemeta for movies matching a search query.
pub async fn search_movies(
    client: &Client,
    query: &str,
    limit: usize,
) -> Result<Vec<CinemetaSearchItem>, String> {
    let clean = query.trim();
    if clean.is_empty() {
        return Ok(Vec::new());
    }

    let encoded: String = urlencoding::encode(clean).into_owned();
    let url = format!("{CINEMETA_BASE_URL}/catalog/movie/top/search={encoded}.json");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Cinemeta HTTP error: {e}"))?;

    let catalog: CatalogResponse = resp
        .json()
        .await
        .map_err(|e| format!("Cinemeta JSON decode error: {e}"))?;

    Ok(catalog.metas.into_iter().take(limit).collect())
}

/// Searches Cinemeta for TV series matching a search query.
pub async fn search_series(
    client: &Client,
    query: &str,
    limit: usize,
) -> Result<Vec<CinemetaSearchItem>, String> {
    let clean = query.trim();
    if clean.is_empty() {
        return Ok(Vec::new());
    }

    let encoded: String = urlencoding::encode(clean).into_owned();
    let url = format!("{CINEMETA_BASE_URL}/catalog/series/top/search={encoded}.json");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Cinemeta HTTP error: {e}"))?;

    let catalog: CatalogResponse = resp
        .json()
        .await
        .map_err(|e| format!("Cinemeta JSON decode error: {e}"))?;

    Ok(catalog.metas.into_iter().take(limit).collect())
}

/// Fetches full movie metadata by IMDb ID (e.g. "tt1375666").
pub async fn get_movie_by_id(client: &Client, id: &str) -> Result<Option<CinemetaMedia>, String> {
    let url = format!("{CINEMETA_BASE_URL}/meta/movie/{id}.json");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Cinemeta HTTP error: {e}"))?;

    let body: MetaResponse = resp
        .json()
        .await
        .map_err(|e| format!("Cinemeta JSON decode error: {e}"))?;

    Ok(body.meta)
}

/// Fetches full TV series metadata by IMDb ID (e.g. "tt0903747").
pub async fn get_series_by_id(client: &Client, id: &str) -> Result<Option<CinemetaMedia>, String> {
    let url = format!("{CINEMETA_BASE_URL}/meta/series/{id}.json");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Cinemeta HTTP error: {e}"))?;

    let body: MetaResponse = resp
        .json()
        .await
        .map_err(|e| format!("Cinemeta JSON decode error: {e}"))?;

    Ok(body.meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_item_display_label() {
        let item = CinemetaSearchItem {
            id: "tt1375666".to_string(),
            media_type: "movie".to_string(),
            name: "Inception".to_string(),
            poster: None,
            release_info: Some("2010".to_string()),
            year: None,
        };

        assert_eq!(item.display_label(), "Inception (2010)");
    }
}

