use crate::{
    Error, Result,
    cache::atomic_write,
    model::{SkippedItem, Track},
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub index: usize,
    #[serde(flatten)]
    pub track: Track,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub actual_file: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub bandwidth_hz: u64,
    #[serde(default)]
    pub quality_rating: String,
    #[serde(default)]
    pub gain_offset_db: f64,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub error: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub mix_title: String,
    pub target_dir: String,
    pub created_at: String,
    pub updated_at: String,
    pub tracks: Vec<Entry>,
    #[serde(default)]
    pub skipped: Vec<SkippedItem>,
}
impl Manifest {
    pub fn load_or_create(
        directory: &Path,
        title: &str,
        tracks: &[Track],
        skipped: &[SkippedItem],
    ) -> Result<Self> {
        match std::fs::read(directory.join("mix_manifest.json")) {
            Ok(bytes) => {
                let manifest: Self = serde_json::from_slice(&bytes)?;
                if manifest.mix_title != title
                    || manifest.tracks.len() != tracks.len()
                    || manifest
                        .tracks
                        .iter()
                        .zip(tracks)
                        .enumerate()
                        .any(|(i, (a, b))| {
                            a.index != i + 1
                                || a.track != *b
                                || (!a.actual_file.is_empty() && !safe_name(&a.actual_file))
                        })
                {
                    return Err(Error::Invalid("existing mix manifest differs from this tracklist; choose another output directory".into()));
                }
                Ok(manifest)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let now = chrono::Utc::now().to_rfc3339();
                let target_dir = directory
                    .to_str()
                    .ok_or_else(|| Error::Invalid("manifest directory is not UTF-8".into()))?
                    .into();
                Ok(Self {
                    mix_title: title.into(),
                    target_dir,
                    created_at: now.clone(),
                    updated_at: now,
                    tracks: tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| Entry {
                            index: i + 1,
                            track: t.clone(),
                            status: "pending".into(),
                            actual_file: String::new(),
                            duration: 0.0,
                            bandwidth_hz: 0,
                            quality_rating: String::new(),
                            gain_offset_db: 0.0,
                            source_url: String::new(),
                            error: String::new(),
                        })
                        .collect(),
                    skipped: skipped.to_vec(),
                })
            }
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, directory: &Path) -> Result<()> {
        let mut manifest = self.clone();
        manifest.updated_at = chrono::Utc::now().to_rfc3339();
        atomic_write(
            &directory.join("mix_manifest.json"),
            &serde_json::to_vec_pretty(&manifest)?,
        )
    }
}
pub fn safe_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_)))
        && parts.next().is_none()
        && !name.contains(['\n', '\r', '\\'])
}
