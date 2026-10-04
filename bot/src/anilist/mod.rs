use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::warn;

pub const ANILIST_API_URL: &str = "https://graphql.anilist.co";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimeMedia {
    pub id: i32,
    pub title: AnimeTitle,
    pub format: Option<String>,
    pub status: Option<String>,
    pub episodes: Option<i32>,
    #[serde(rename = "seasonYear")]
    pub season_year: Option<i32>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(rename = "averageScore")]
    pub average_score: Option<i32>,
    pub description: Option<String>,
    #[serde(rename = "siteUrl")]
    pub site_url: Option<String>,
    #[serde(rename = "coverImage")]
    pub cover_image: Option<AnimeCoverImage>,
    #[serde(rename = "bannerImage")]
    pub banner_image: Option<String>,
}

impl AnimeMedia {
    /// Preferred display title (English if available, otherwise Romaji).
    pub fn display_title(&self) -> &str {
        if let Some(ref eng) = self.title.english {
            if !eng.trim().is_empty() {
                return eng;
            }
        }
        &self.title.romaji
    }

    /// Clean synopsis without HTML tags or entities.
    pub fn clean_description(&self) -> String {
        match &self.description {
            Some(desc) => clean_html(desc),
            None => "No synopsis available.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimeTitle {
    pub romaji: String,
    pub english: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimeCoverImage {
    #[serde(rename = "extraLarge")]
    pub extra_large: Option<String>,
    pub large: Option<String>,
    pub medium: Option<String>,
}

impl AnimeCoverImage {
    pub fn best_url(&self) -> Option<&str> {
        self.extra_large
            .as_deref()
            .or(self.large.as_deref())
            .or(self.medium.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimeSearchMatch {
    pub id: i32,
    pub title: AnimeTitle,
    pub format: Option<String>,
    #[serde(rename = "seasonYear")]
    pub season_year: Option<i32>,
    pub episodes: Option<i32>,
}

impl AnimeSearchMatch {
    pub fn display_label(&self) -> String {
        let title = self.title.english.as_deref().unwrap_or(&self.title.romaji);
        let mut meta = Vec::new();
        if let Some(ref fmt) = self.format {
            meta.push(fmt.clone());
        }
        if let Some(yr) = self.season_year {
            meta.push(yr.to_string());
        }
        if meta.is_empty() {
            title.chars().take(95).collect()
        } else {
            let label = format!("{} ({})", title, meta.join(", "));
            label.chars().take(95).collect()
        }
    }
}

#[derive(Serialize)]
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: serde_json::Value,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct GraphQlError {
    message: String,
}

#[derive(Deserialize)]
struct SearchData {
    #[serde(rename = "Page")]
    page: PageData,
}

#[derive(Deserialize)]
struct PageData {
    media: Vec<AnimeSearchMatch>,
}

#[derive(Deserialize)]
struct MediaData {
    #[serde(rename = "Media")]
    media: Option<AnimeMedia>,
}

/// Searches AniList for anime matching a partial string (for autocomplete and search).
pub async fn search_anime(client: &Client, search: &str, limit: u32) -> Result<Vec<AnimeSearchMatch>, String> {
    let query = r#"
    query ($search: String, $perPage: Int) {
      Page(page: 1, perPage: $perPage) {
        media(search: $search, type: ANIME, sort: SEARCH_MATCH) {
          id
          title {
            romaji
            english
          }
          format
          seasonYear
          episodes
        }
      }
    }
    "#;

    let payload = GraphQlRequest {
        query,
        variables: serde_json::json!({
            "search": search,
            "perPage": limit,
        }),
    };

    let resp = client
        .post(ANILIST_API_URL)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("HTTP request error: {e}"))?;

    let body: GraphQlResponse<SearchData> = resp
        .json()
        .await
        .map_err(|e| format!("JSON decode error: {e}"))?;

    if let Some(errors) = body.errors {
        if !errors.is_empty() {
            warn!("AniList GraphQL errors: {:?}", errors);
        }
    }

    Ok(body.data.map(|d| d.page.media).unwrap_or_default())
}

/// Fetches full anime details by AniList media ID.
pub async fn get_anime_by_id(client: &Client, id: i32) -> Result<Option<AnimeMedia>, String> {
    let query = r#"
    query ($id: Int) {
      Media(id: $id, type: ANIME) {
        id
        title {
          romaji
          english
        }
        format
        status
        episodes
        seasonYear
        genres
        averageScore
        description(asHtml: false)
        siteUrl
        coverImage {
          extraLarge
          large
          medium
        }
        bannerImage
      }
    }
    "#;

    let payload = GraphQlRequest {
        query,
        variables: serde_json::json!({ "id": id }),
    };

    let resp = client
        .post(ANILIST_API_URL)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("HTTP request error: {e}"))?;

    let body: GraphQlResponse<MediaData> = resp
        .json()
        .await
        .map_err(|e| format!("JSON decode error: {e}"))?;

    Ok(body.data.and_then(|d| d.media))
}

/// Fetches anime details by searching for a title.
pub async fn get_anime_by_title(client: &Client, search: &str) -> Result<Option<AnimeMedia>, String> {
    let query = r#"
    query ($search: String) {
      Media(search: $search, type: ANIME, sort: SEARCH_MATCH) {
        id
        title {
          romaji
          english
        }
        format
        status
        episodes
        seasonYear
        genres
        averageScore
        description(asHtml: false)
        siteUrl
        coverImage {
          extraLarge
          large
          medium
        }
        bannerImage
      }
    }
    "#;

    let payload = GraphQlRequest {
        query,
        variables: serde_json::json!({ "search": search }),
    };

    let resp = client
        .post(ANILIST_API_URL)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("HTTP request error: {e}"))?;

    let body: GraphQlResponse<MediaData> = resp
        .json()
        .await
        .map_err(|e| format!("JSON decode error: {e}"))?;

    Ok(body.data.and_then(|d| d.media))
}

/// Cleans HTML tags and entities out of AniList text descriptions.
pub fn clean_html(input: &str) -> String {
    let replaced = input
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<i>", "*")
        .replace("</i>", "*")
        .replace("<b>", "**")
        .replace("</b>", "**")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&mdash;", "—")
        .replace("&#039;", "'");

    let mut result = String::with_capacity(replaced.len());
    let mut in_tag = false;

    for ch in replaced.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_html() {
        let dirty = "A story about <i>time travel</i>.<br><br><b>Warning:</b> &quot;Mad scientist!&quot;";
        let cleaned = clean_html(dirty);
        assert_eq!(
            cleaned,
            "A story about *time travel*.\n\n**Warning:** \"Mad scientist!\""
        );
    }
}
