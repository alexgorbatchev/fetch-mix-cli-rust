use crate::{
    CancellationToken, Error, Result,
    cache::Cache,
    llm::Llm,
    model::{SkippedItem, Track},
    process,
};
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, path::Path};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub like_count: i64,
}
#[derive(Serialize, Deserialize)]
pub struct Video {
    pub title: String,
    #[serde(default)]
    pub comments: Vec<Comment>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedTracks {
    pub video_id: String,
    pub video_title: String,
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub skipped_items: Vec<SkippedItem>,
}
pub fn video_id(input: &str) -> Option<String> {
    let valid = |s: &str| {
        s.len() == 11
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    };
    let input = input.trim();
    if valid(input) {
        return Some(input.into());
    }
    let url = reqwest::Url::parse(input).ok()?;
    let id = match url.host_str()? {
        "youtu.be" => url.path().trim_start_matches('/').to_owned(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => {
            if url.path() == "/watch" {
                url.query_pairs().find(|(k, _)| k == "v")?.1.into_owned()
            } else {
                let mut parts = url.path().trim_matches('/').split('/');
                if !matches!(parts.next()?, "shorts" | "embed" | "v" | "e") {
                    return None;
                }
                parts.next()?.into()
            }
        }
        _ => return None,
    };
    valid(&id).then_some(id)
}
pub fn filter_comments(comments: &[Comment]) -> Vec<Comment> {
    let mut candidates: Vec<_> = comments
        .iter()
        .filter(|c| {
            c.text.lines().count() >= 3
                && c.text
                    .chars()
                    .filter(|c| matches!(c, '-' | '‒' | '–' | '—' | '―' | ':'))
                    .count()
                    >= 3
                && c.text.bytes().filter(u8::is_ascii_digit).count() >= 5
        })
        .cloned()
        .collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.like_count));
    candidates
}
pub async fn process(
    input: &str,
    executable: &Path,
    path: Option<&OsStr>,
    cache: Option<&Cache>,
    llm: Option<&Llm>,
    token: &CancellationToken,
) -> Result<CachedTracks> {
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let id = video_id(input)
        .ok_or_else(|| Error::Invalid("expected a YouTube URL or 11-character video ID".into()))?;
    let key = format!("yt_tracks_{id}.json");
    if let Some(cached) = cache
        .and_then(|c| c.get::<CachedTracks>(&key))
        .filter(|c| !c.tracks.is_empty())
    {
        return Ok(cached);
    }
    let comment_key = format!("yt_cache_{id}.json");
    let info = if let Some(cached) = cache.and_then(|c| c.get::<Video>(&comment_key)) {
        cached
    } else {
        let args = [
            "--no-playlist",
            "--write-comments",
            "--skip-download",
            "--dump-json",
            "--extractor-args",
            "youtube:max-comments=200,comment_sort=top",
            "--js-runtimes",
            "node",
            "--no-update",
        ]
        .map(str::to_owned)
        .into_iter()
        .chain([format!("https://www.youtube.com/watch?v={id}")])
        .collect::<Vec<_>>();
        let output = process::capture(executable, &args, path, token).await?;
        if !output.status.success() {
            return Err(Error::Invalid(format!(
                "yt-dlp comment extraction failed: {}",
                cli_deps::sanitize_stderr(&String::from_utf8_lossy(&output.stderr))
            )));
        }
        let info: Video = serde_json::from_slice(&output.stdout)?;
        if let Some(cache) = cache {
            cache.put(&comment_key, &info)?
        }
        info
    };
    let candidates = filter_comments(&info.comments);
    if candidates.is_empty() {
        return Err(Error::Invalid(
            "no comments found that look like a tracklist".into(),
        ));
    }
    let llm = llm.ok_or_else(|| {
        Error::Invalid("YouTube comment extraction requires a configured AI provider".into())
    })?;
    let extraction = llm
        .extract_comments(&serde_json::to_value(candidates)?, token)
        .await?;
    if !extraction.found || extraction.tracks.is_empty() {
        return Err(Error::Invalid(
            "AI could not identify a valid tracklist in the candidate comments".into(),
        ));
    }
    let result = CachedTracks {
        video_id: id,
        video_title: info.title,
        tracks: extraction.tracks,
        skipped_items: extraction.skipped_items,
    };
    if let Some(cache) = cache {
        cache.put(&key, &result)?
    }
    Ok(result)
}
