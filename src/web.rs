use crate::{CancellationToken, Error, Result, cache::Cache, cancellable, model::SearchResult};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::time::Duration;
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub mixes_db: String,
    pub mirrors: Vec<String>,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            mixes_db: "https://www.mixesdb.com/".into(),
            mirrors: vec![
                "https://www.openingtrack.com/wp-json/wp/v2/posts".into(),
                "https://tracklist.club/wp-json/wp/v2/posts".into(),
            ],
        }
    }
}
pub struct Discovery {
    client: Client,
    endpoints: Endpoints,
}
#[derive(Serialize, Deserialize)]
struct CachedMarkdown {
    url: String,
    markdown: String,
}
fn url(input: &str) -> Result<Url> {
    let url = Url::parse(input).map_err(|e| Error::Invalid(e.to_string()))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(Error::Invalid("expected an HTTP URL".into()));
    }
    Ok(url)
}
impl Discovery {
    pub fn new(endpoints: Endpoints) -> Result<Self> {
        url(&endpoints.mixes_db)?;
        for endpoint in &endpoints.mirrors {
            url(endpoint)?;
        }
        Ok(Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(Duration::from_secs(20))
                .build()?,
            endpoints,
        })
    }
    async fn text(&self, url: Url, token: &CancellationToken) -> Result<String> {
        cancellable(token, async {
            Ok(self
                .client
                .get(url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?)
        })
        .await
    }
    pub async fn mixes_db(
        &self,
        query: &str,
        token: &CancellationToken,
    ) -> Result<Vec<SearchResult>> {
        let base = url(&self.endpoints.mixes_db)?;
        let mut api = base
            .join("w/api.php")
            .map_err(|e| Error::Invalid(e.to_string()))?;
        api.query_pairs_mut().extend_pairs([
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("format", "json"),
        ]);
        #[derive(Deserialize)]
        struct Search {
            title: String,
        }
        #[derive(Deserialize)]
        struct Query {
            search: Vec<Search>,
        }
        #[derive(Deserialize)]
        struct Response {
            query: Query,
        }
        let response: Response = serde_json::from_str(&self.text(api, token).await?)?;
        response
            .query
            .search
            .into_iter()
            .map(|item| {
                let mut page = base.clone();
                page.path_segments_mut()
                    .map_err(|_| Error::Invalid("invalid MixesDB base".into()))?
                    .pop_if_empty()
                    .extend(["w", &item.title.replace(' ', "_")]);
                Ok(SearchResult {
                    title: item.title,
                    url: page.to_string(),
                })
            })
            .collect()
    }
    pub async fn mirrors(
        &self,
        query: &str,
        token: &CancellationToken,
    ) -> Result<Vec<SearchResult>> {
        let mut results = Vec::new();
        #[derive(Deserialize)]
        struct Title {
            rendered: String,
        }
        #[derive(Deserialize)]
        struct Post {
            link: String,
            title: Title,
        }
        for endpoint in &self.endpoints.mirrors {
            let mut api = url(endpoint)?;
            api.query_pairs_mut()
                .extend_pairs([("search", query), ("per_page", "5")]);
            match self.text(api, token).await {
                Ok(text) => {
                    if let Ok(posts) = serde_json::from_str::<Vec<Post>>(&text) {
                        results.extend(posts.into_iter().map(|p| SearchResult {
                            url: p.link,
                            title:
                                html_escape::decode_html_entities(&p.title.rendered).into_owned(),
                        }))
                    }
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(_) => {}
            }
        }
        Ok(results)
    }
    pub async fn search(
        &self,
        query: &str,
        token: &CancellationToken,
    ) -> Result<Vec<SearchResult>> {
        let query = search_query(query);
        match self.mixes_db(&query, token).await {
            Ok(results) if !results.is_empty() => return Ok(results),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            _ => {}
        }
        self.mirrors(&query, token).await
    }
    pub async fn scrape(
        &self,
        input: &str,
        cache: Option<&Cache>,
        token: &CancellationToken,
    ) -> Result<String> {
        if token.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let key = format!("scrape_{}.json", Cache::hash(input));
        if let Some(cached) = cache
            .and_then(|c| c.get::<CachedMarkdown>(&key))
            .filter(|c| !c.markdown.is_empty())
        {
            return Ok(cached.markdown);
        }
        let page = url(input)?;
        let base = url(&self.endpoints.mixes_db)?;
        let raw = if page.host_str() == base.host_str()
            && page.port_or_known_default() == base.port_or_known_default()
            && page.path().starts_with("/w/")
        {
            let mut raw = page.clone();
            raw.set_path("/w/index.php");
            raw.set_query(None);
            raw.query_pairs_mut().extend_pairs([
                (
                    "title",
                    percent_encoding::percent_decode_str(page.path().trim_start_matches("/w/"))
                        .decode_utf8_lossy()
                        .as_ref(),
                ),
                ("action", "raw"),
            ]);
            match self.text(raw, token).await {
                Ok(text)
                    if !text.trim().is_empty()
                        && !text.trim().to_lowercase().starts_with("<!doctype") =>
                {
                    Some(text)
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                _ => None,
            }
        } else {
            None
        };
        let markdown = if let Some(raw) = raw {
            raw
        } else {
            htmd::HtmlToMarkdown::builder()
                .skip_tags(vec!["script", "style", "nav"])
                .build()
                .convert(&self.text(page, token).await?)?
                .trim()
                .to_owned()
        };
        if let Some(cache) = cache {
            cache.put(
                &key,
                &CachedMarkdown {
                    url: input.into(),
                    markdown: markdown.clone(),
                },
            )?
        }
        Ok(markdown)
    }
}
pub fn search_query(input: &str) -> String {
    if let Ok(url) = Url::parse(input)
        && url
            .host_str()
            .is_some_and(|h| h == "1001tracklists.com" || h == "www.1001tracklists.com")
        && url.path().starts_with("/tracklist/")
    {
        let slug = url
            .path_segments()
            .and_then(|mut s| s.rfind(|s| !s.is_empty()))
            .unwrap_or("");
        let slug = if slug.to_lowercase().ends_with(".html") {
            &slug[..slug.len() - 5]
        } else {
            slug
        };
        return slug.replace('-', " ");
    }
    input.into()
}
pub fn title_from_url(input: &str) -> String {
    let slug = Url::parse(input)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut s| s.rfind(|s| !s.is_empty()))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let slug = percent_encoding::percent_decode_str(&slug).decode_utf8_lossy();
    let slug = slug
        .trim_end_matches(".html")
        .trim_end_matches(".htm")
        .replace(['_', '-'], " ");
    if slug.is_empty() {
        "Tracklist".into()
    } else {
        slug.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}
