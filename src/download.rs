use crate::{
    CancellationToken, Error, Result,
    cache::atomic_write,
    manifest::{Manifest, safe_name},
    model::{SkippedItem, Track},
    process,
    progress::{Event, Reporter, Server},
    ui::Ui,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Clone)]
pub struct DownloadOptions {
    pub title: String,
    pub tracks: Vec<Track>,
    pub skipped: Vec<SkippedItem>,
    pub output: Option<PathBuf>,
    pub dry_run: bool,
    pub sources: String,
    pub skip_verify: bool,
    pub skip_metadata: bool,
    pub verbose: bool,
    pub reporter: Option<Reporter>,
    pub executable: PathBuf,
    pub path: Option<OsString>,
    pub socket_dir: PathBuf,
    pub force_tcp: bool,
    pub delay: Duration,
}
impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            tracks: Vec::new(),
            skipped: Vec::new(),
            output: None,
            dry_run: false,
            sources: "youtube,soundcloud".into(),
            skip_verify: false,
            skip_metadata: false,
            verbose: false,
            reporter: None,
            executable: "fetch-track".into(),
            path: None,
            socket_dir: ".tmp".into(),
            force_tcp: false,
            delay: Duration::from_secs(1),
        }
    }
}
pub fn sanitize(input: &str) -> String {
    let mut output = String::new();
    let mut invalid = false;
    for ch in input.chars() {
        if ch.is_control() || "/\\?%*:|\"<>".contains(ch) {
            if !invalid {
                output.push('-')
            }
            invalid = true
        } else {
            output.push(ch);
            invalid = false
        }
    }
    let name = output.trim();
    if name.is_empty() || matches!(name, "." | "..") {
        "Downloaded DJ Set".into()
    } else {
        name.into()
    }
}
pub fn target_dir(options: &DownloadOptions) -> PathBuf {
    options
        .output
        .as_ref()
        .filter(|p| p.as_os_str() != "" && p.as_path() != Path::new("."))
        .cloned()
        .unwrap_or_else(|| PathBuf::from(sanitize(&options.title)))
}
fn track_info(track: &Track, index: usize, total: usize, status: &str) -> Value {
    json!({"index":index,"total_tracks":total,"artist":track.artist,"title":track.title,"timestamp":track.timestamp,"status":status})
}
async fn emit(options: &DownloadOptions, event: Event, token: &CancellationToken) -> Result<()> {
    if let Some(reporter) = &options.reporter {
        reporter.emit(event.stamped(), token).await?
    }
    Ok(())
}
fn summary(options: &DownloadOptions, directory: &Path, downloaded: usize, failed: usize) -> Value {
    json!({"mix_title":options.title,"target_dir":Ui::path(directory),"playlist_path":Ui::path(&directory.join("playlist.m3u")),"total_tracks":options.tracks.len(),"downloaded":downloaded,"failed":failed,"skipped":options.skipped.len()})
}
pub async fn download(options: &DownloadOptions, ui: &Ui, token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let directory = target_dir(options);
    let total = options.tracks.len();
    let hours = options
        .tracks
        .iter()
        .map(|t| t.timestamp.as_str())
        .chain(options.skipped.iter().map(|t| t.timestamp.as_str()))
        .any(|t| {
            t.split(':').count() == 3
                && t.split(':')
                    .next()
                    .and_then(|h| h.parse::<u64>().ok())
                    .is_some_and(|h| h > 0)
        });
    if options.dry_run {
        ui.status("status", "dry_run")?;
        ui.status("target_dir", Ui::path(&directory))?;
        ui.status("playlist", Ui::path(&directory.join("playlist.m3u")))?;
        ui.status("total_tracks", total)?;
        let mut event = Event::new("phase_start", "dry_run");
        event.summary = Some(summary(options, &directory, 0, 0));
        emit(options, event, token).await?;
        for (i, track) in options.tracks.iter().enumerate() {
            let timestamp = crate::parser::normalize_timestamp(&track.timestamp, hours);
            ui.line(format!(
                "index:{:02}\tartist:{}\ttitle:{}\ttimestamp:{timestamp}",
                i + 1,
                track.artist,
                track.title
            ))?;
            let mut event = Event::new("track_start", "dry_run");
            event.step = Some(i + 1);
            event.total_steps = Some(total);
            event.track = Some(track_info(track, i + 1, total, "planned"));
            emit(options, event, token).await?;
        }
        for skipped in &options.skipped {
            ui.line(format!(
                "skipped: {}\treason:{}\ttimestamp:{}",
                skipped.raw_text,
                skipped.reason,
                crate::parser::normalize_timestamp(&skipped.timestamp, hours)
            ))?
        }
        let mut event = Event::new("complete", "dry_run");
        event.summary = Some(summary(options, &directory, 0, 0));
        return emit(options, event, token).await;
    }
    std::fs::create_dir_all(&directory)?;
    let mut manifest = Manifest::load_or_create(
        &directory,
        &options.title,
        &options.tracks,
        &options.skipped,
    )?;
    manifest.save(&directory)?;
    ui.status("target_dir", Ui::path(&directory))?;
    let mut start = Event::new("phase_start", "download");
    start.total_steps = Some(total);
    emit(options, start, token).await?;
    let mut completed = 0;
    let mut failed = 0;
    for (i, track) in options.tracks.iter().enumerate() {
        if token.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let entry = &manifest.tracks[i];
        if entry.status == "completed"
            && safe_name(&entry.actual_file)
            && regular_nonempty(&directory.join(&entry.actual_file))
        {
            completed += 1;
            ui.line(format!(
                "[{:02}/{total:02}] Skipping {}",
                i + 1,
                entry.actual_file
            ))?;
            let mut event = Event::new("track_complete", "download");
            let mut info = track_info(track, i + 1, total, "skipped");
            info["actual_file"] = entry.actual_file.clone().into();
            event.track = Some(info);
            emit(options, event, token).await?;
            continue;
        }
        ui.line(format!(
            "[{:02}/{total:02}] Fetching {} - {}",
            i + 1,
            track.artist,
            track.title
        ))?;
        let mut start = Event::new("track_start", "download");
        start.step = Some(i + 1);
        start.total_steps = Some(total);
        start.track = Some(track_info(track, i + 1, total, "pending"));
        emit(options, start, token).await?;
        let outcome = download_one(options, track, i + 1, &directory, ui, token).await;
        let entry = &mut manifest.tracks[i];
        let mut event = Event::new("track_complete", "download");
        event.step = Some(i + 1);
        event.total_steps = Some(total);
        match outcome {
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(error) => {
                failed += 1;
                entry.status = "failed".into();
                entry.actual_file.clear();
                entry.error = error.to_string();
                ui.line(format!("track_failed: {}", entry.error))?;
                let mut info = track_info(track, i + 1, total, "failed");
                info["error"] = entry.error.clone().into();
                event.track = Some(info);
            }
            Ok((file, result, source)) => {
                completed += 1;
                entry.status = "completed".into();
                entry.error.clear();
                entry.actual_file = file.clone();
                entry.source_url = source;
                entry.duration = result["duration_seconds"].as_f64().unwrap_or_default();
                entry.bandwidth_hz = result["bandwidth_hz"].as_u64().unwrap_or_default();
                entry.quality_rating = result["bandwidth_rating"]
                    .as_str()
                    .unwrap_or_default()
                    .into();
                entry.gain_offset_db = result["suggested_gain_db"].as_f64().unwrap_or_default();
                let mut info = track_info(track, i + 1, total, "completed");
                info["actual_file"] = file.into();
                for key in [
                    "duration_seconds",
                    "bandwidth_hz",
                    "bandwidth_rating",
                    "suggested_gain_db",
                ] {
                    if let Some(value) = result.get(key) {
                        info[key] = value.clone();
                    }
                }
                event.track = Some(info);
                event.result = Some(result);
                ui.line(format!(
                    "track_completed: {}\tduration:{}\tquality:{}\tgain_db:{}",
                    entry.actual_file,
                    crate::parser::format_duration(entry.duration),
                    entry.quality_rating,
                    entry.gain_offset_db
                ))?;
            }
        }
        manifest.save(&directory)?;
        emit(options, event, token).await?;
        if i + 1 < total && !options.delay.is_zero() {
            let jitter = Duration::from_millis(fastrand::u64(0..1500));
            crate::cancellable(token, async {
                tokio::time::sleep(options.delay + jitter).await;
                Ok(())
            })
            .await?;
        }
    }
    generate_playlist(&directory, &options.tracks, &manifest)?;
    ui.status("downloaded", completed)?;
    ui.status("failed", failed)?;
    ui.status("playlist", Ui::path(&directory.join("playlist.m3u")))?;
    let mut event = Event::new("complete", "complete");
    event.summary = Some(summary(options, &directory, completed, failed));
    emit(options, event, token).await?;
    if failed > 0 {
        Err(Error::Partial { failed, total })
    } else {
        Ok(())
    }
}
fn regular_nonempty(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}
fn files(directory: &Path) -> Result<BTreeSet<String>> {
    Ok(std::fs::read_dir(directory)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            !n.starts_with('.') && !matches!(n.as_str(), "playlist.m3u" | "mix_manifest.json")
        })
        .collect())
}
#[derive(Default)]
struct ChildResult {
    result: Value,
    source: String,
    error: String,
}
async fn forward(
    options: &DownloadOptions,
    mut event: Event,
    context: &Value,
    state: &mut ChildResult,
    ui: &Ui,
    token: &CancellationToken,
) -> Result<()> {
    match event.kind.as_str() {
        "complete" => {
            if let Some(value) = &event.result {
                state.result = value.clone()
            }
        }
        "candidate_selected" => {
            if let Some(candidate) = &event.candidate {
                state.source = candidate["url"].as_str().unwrap_or_default().into();
                if ui.mode == cli_command_kit::Mode::Human {
                    ui.line(format!(
                        "  Selected: {}",
                        candidate["title"].as_str().unwrap_or_default()
                    ))?
                }
            }
        }
        "error" => state.error = event.error.clone(),
        _ => {}
    }
    if event.track.is_none() {
        event.track = Some(context.clone())
    }
    emit(options, event, token).await
}
async fn download_one(
    options: &DownloadOptions,
    track: &Track,
    index: usize,
    directory: &Path,
    ui: &Ui,
    token: &CancellationToken,
) -> Result<(String, Value, String)> {
    let before = files(directory)?;
    let mut server = Server::start(&options.socket_dir, options.force_tcp, token).await?;
    let query = if track.raw_string.is_empty() {
        format!("{} - {}", track.artist, track.title)
    } else {
        track.raw_string.clone()
    };
    let mut args = vec![
        query,
        "--out-dir".into(),
        directory
            .to_str()
            .ok_or_else(|| Error::Invalid("output directory is not UTF-8".into()))?
            .into(),
    ];
    if !options.sources.is_empty() {
        args.extend(["--sources".into(), options.sources.clone()]);
    }
    for (flag, enabled) in [
        ("--skip-verify", options.skip_verify),
        ("--skip-metadata", options.skip_metadata),
        ("--verbose", options.verbose),
    ] {
        if enabled {
            args.push(flag.into())
        }
    }
    args.extend(["--progress-target".into(), server.target.clone()]);
    let mut state = ChildResult::default();
    let context = track_info(track, index, options.tracks.len(), "pending");
    let mut live = crate::ui::LiveOutput::new(ui);
    let display = options.verbose || ui.mode == cli_command_kit::Mode::Agent;
    let output = {
        let capture = process::capture_with(
            &options.executable,
            &args,
            options.path.as_deref(),
            token,
            |bytes, stderr| {
                if display {
                    live.feed(bytes, stderr)
                } else {
                    Ok(())
                }
            },
        );
        tokio::pin!(capture);
        loop {
            tokio::select! {output=&mut capture=>break output?,event=server.next()=>if let Some(event)=event{forward(options,event?,&context,&mut state,ui,token).await?}}
        }
    };
    for event in server.finish().await? {
        forward(options, event, &context, &mut state, ui, token).await?
    }
    if display {
        live.finish()?
    }
    let ChildResult {
        mut result,
        source,
        mut error,
    } = state;
    if !output.status.success() {
        if error.is_empty() {
            error = cli_deps::sanitize_stderr(&String::from_utf8_lossy(&output.stderr));
        }
        return Err(Error::Invalid(format!(
            "fetch-track {}: {}",
            output.status, error
        )));
    }
    let file = if let Some(path) = result["path"].as_str().filter(|s| !s.is_empty()) {
        let path = if let Some(tail) = path.strip_prefix("~/") {
            dirs::home_dir()
                .ok_or_else(|| Error::Invalid("home directory unavailable".into()))?
                .join(tail)
        } else {
            PathBuf::from(path)
        };
        let canonical = std::fs::canonicalize(&path)?;
        if canonical.parent() != Some(std::fs::canonicalize(directory)?.as_path()) {
            return Err(Error::Invalid(
                "fetch-track artifact is outside the output directory".into(),
            ));
        }
        canonical
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Invalid("artifact filename is not UTF-8".into()))?
            .to_owned()
    } else {
        let after = files(directory)?;
        let new: Vec<_> = after.difference(&before).cloned().collect();
        if new.len() != 1 {
            return Err(Error::Invalid(format!(
                "fetch-track succeeded without one identifiable artifact ({} new files)",
                new.len()
            )));
        }
        new[0].clone()
    };
    if !safe_name(&file) || !regular_nonempty(&directory.join(&file)) {
        return Err(Error::Invalid(
            "fetch-track artifact is not a nonempty regular file".into(),
        ));
    }
    let prefix = format!("{index:02} - ");
    let numbered = if file.starts_with(&prefix) {
        file.clone()
    } else {
        format!("{prefix}{file}")
    };
    if numbered != file {
        if directory.join(&numbered).exists() {
            return Err(Error::Invalid(
                "numbered artifact already exists; refusing to overwrite".into(),
            ));
        }
        std::fs::rename(directory.join(&file), directory.join(&numbered))?;
    }
    if result.is_null() {
        result = json!({})
    }
    result["path"] = Ui::path(&directory.join(&numbered)).into();
    Ok((numbered, result, source))
}
pub fn generate_playlist(
    directory: &Path,
    tracks: &[Track],
    manifest: &Manifest,
) -> Result<PathBuf> {
    let available = files(directory)?;
    let mut playlist = String::from("#EXTM3U\n");
    for (i, track) in tracks.iter().enumerate() {
        let entry = manifest.tracks.get(i);
        let prefix = format!("{:02} - ", i + 1);
        let actual = entry
            .filter(|e| {
                e.status == "completed"
                    && safe_name(&e.actual_file)
                    && regular_nonempty(&directory.join(&e.actual_file))
            })
            .map(|e| e.actual_file.clone())
            .or_else(|| available.iter().find(|n| n.starts_with(&prefix)).cloned())
            .unwrap_or_else(|| {
                format!(
                    "{prefix}{} - {}",
                    sanitize(&track.artist),
                    sanitize(&track.title)
                )
            });
        let label = format!("{} - {}", track.artist, track.title).replace(['\n', '\r'], " ");
        playlist.push_str(&format!("#EXTINF:-1,{label}\n{actual}\n"));
    }
    let path = directory.join("playlist.m3u");
    atomic_write(&path, playlist.as_bytes())?;
    Ok(path)
}
