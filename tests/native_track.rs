#![cfg(unix)]
mod common;
use fetch_mix_cli::{manifest::Manifest, model::Track};
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
};
fn native_tool(name: &str) -> PathBuf {
    let path = which::which(name).unwrap();
    if let Ok(script) = std::fs::read_to_string(&path)
        && let Some(value) = script.lines().find_map(|line| {
            line.strip_prefix("TOOL_EXECUTABLE=\"")
                .and_then(|value| value.strip_suffix('"'))
        })
    {
        let executable = PathBuf::from(value);
        assert!(executable.is_file());
        return executable;
    }
    path
}
#[tokio::test]
#[ignore = "requires a separately built native Rust fetch-track and installed ffmpeg/ffprobe"]
async fn real_rust_track_native_audio_numbering_and_resume() {
    let executable = PathBuf::from(
        std::env::var_os("FETCH_MIX_NATIVE_TRACK")
            .expect("FETCH_MIX_NATIVE_TRACK points to native Rust binary"),
    );
    assert_eq!(
        Command::new(&executable)
            .arg("--version")
            .output()
            .unwrap()
            .stdout,
        b"0.1.0\n"
    );
    let directory = tempfile::tempdir_in(".tmp").unwrap();
    let root = directory.path().canonicalize().unwrap();
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let audio = root.join("source.flac");
    let ffmpeg = native_tool("ffmpeg");
    let ffprobe = native_tool("ffprobe");
    assert!(
        Command::new(&ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=14000:sample_rate=48000:duration=1",
                "-y"
            ])
            .arg(&audio)
            .status()
            .unwrap()
            .success()
    );
    symlink(ffmpeg, bin.join("ffmpeg")).unwrap();
    symlink(ffprobe, bin.join("ffprobe")).unwrap();
    symlink(&executable, bin.join("fetch-track")).unwrap();
    let source = format!("'{}'", audio.to_str().unwrap().replace('\'', "'\\''"));
    let script = format!(
        "#!/bin/sh\nset -eu\nmode=download\nprevious=''\npattern=''\nfor argument in \"$@\"; do\ncase \"$argument\" in\n--version) printf '2026.10.01\\n';exit 0;;\n--dump-single-json) mode=probe;;\n--flat-playlist) mode=search;;\nesac\nif [ \"$previous\" = '-o' ];then pattern=$argument;fi\nprevious=$argument\ndone\ncase \"$mode\" in\nprobe) printf '%s\\n' '{{\"title\":\"Artist - Song\",\"artist\":\"Artist\",\"duration\":420,\"ext\":\"flac\"}}';;\nsearch) printf '%s\\n' '{{\"id\":\"native\",\"title\":\"Artist - Song\",\"duration\":420,\"webpage_url\":\"https://example.invalid/audio\"}}';;\ndownload) directory=${{pattern%/*}};target=\"$directory/Artist - Song.flac\";/bin/cp {source} \"$target\";printf '__FETCH_PATH__\"%s\"\\n' \"$target\";;\nesac\n"
    );
    std::fs::write(bin.join("yt-dlp"), script).unwrap();
    std::fs::set_permissions(bin.join("yt-dlp"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let server = common::Http::new(|request| {
        request
            .respond(tiny_http::Response::from_string(
                "<h1>Tracklist</h1><p>Artist - Song</p>",
            ))
            .unwrap()
    });
    let run = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fetch-mix"));
        command
            .current_dir(&root)
            .env_clear()
            .env("PATH", &bin)
            .env("HOME", &root)
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("AGENT", "1")
            .args([
                "--no-cache",
                "--skip-verify",
                "--skip-metadata",
                "--progress-target",
                "stdout",
                "--out-dir",
                "mix",
            ])
            .arg(format!("{}/native-mix", server.url));
        command
    };
    let output = run().output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = Manifest::load_or_create(
        &root.join("mix"),
        "Native Mix",
        &[Track::new("Artist", "Song", "")],
        &[],
    )
    .unwrap();
    assert_eq!(manifest.tracks[0].status, "completed");
    assert_eq!(manifest.tracks[0].actual_file, "01 - Artist - Song.flac");
    assert!(
        std::fs::read(root.join("mix/01 - Artist - Song.flac"))
            .unwrap()
            .starts_with(b"fLaC")
    );
    assert!(
        std::fs::read_to_string(root.join("mix/playlist.m3u"))
            .unwrap()
            .contains("01 - Artist - Song.flac")
    );
    let output = run().output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Skipping"));
}
