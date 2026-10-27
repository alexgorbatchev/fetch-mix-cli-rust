use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub artist: String,
    pub title: String,
    #[serde(default)]
    pub raw_string: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub timestamp: String,
}
impl Track {
    pub fn new(artist: &str, title: &str, timestamp: &str) -> Self {
        let artist = artist.trim().to_owned();
        let title = title.trim().to_owned();
        Self {
            raw_string: format!("{artist} - {title}"),
            artist,
            title,
            timestamp: timestamp.trim().to_owned(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedItem {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub timestamp: String,
    pub raw_text: String,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    #[serde(default)]
    pub found: bool,
    #[serde(default)]
    pub comment_id: String,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub skipped_items: Vec<SkippedItem>,
}
impl Extraction {
    pub fn normalize(&mut self) {
        self.tracks = self
            .tracks
            .iter()
            .filter(|t| !t.artist.trim().is_empty() && !t.title.trim().is_empty())
            .map(|t| Track::new(&t.artist, &t.title, &t.timestamp))
            .collect();
    }
}
