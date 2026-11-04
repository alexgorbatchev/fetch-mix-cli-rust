#![cfg(unix)]
mod common;
use fetch_mix_cli::{CancellationToken, dependencies::Dependencies, llm::Environment};
use std::{os::unix::fs::PermissionsExt, path::Path};

fn tool(directory: &Path, name: &str, version: &str) {
    std::fs::create_dir_all(directory).unwrap();
    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Builds the dependency set for an isolated data directory and PATH.
fn dependencies(data: &Path, global: &Path) -> Dependencies {
    // SAFETY: this test binary runs a single test, and no other thread reads
    // the environment while it changes.
    unsafe {
        std::env::set_var("XDG_DATA_HOME", data);
        std::env::set_var("PATH", format!("{}:/bin:/usr/bin", global.display()));
    }
    Dependencies::new(&Environment::new()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn a_satisfying_global_tool_wins_and_an_outdated_one_falls_back_to_the_managed_copy() {
    let directory = common::scratch();
    let data = directory.path().join("data");
    let managed = data.join("fetch-mix/bin");
    let global = directory.path().join("global");
    let token = CancellationToken::new();
    tool(&managed, "yt-dlp", "2026.01.01");

    tool(&global, "yt-dlp", "2025.01.01");
    let deps = dependencies(&data, &global);
    assert_eq!(
        deps.executable("yt-dlp", &token).await.unwrap(),
        global.join("yt-dlp")
    );
    let path = deps.child_path(&token).await.unwrap();
    let entries = std::env::split_paths(&path).collect::<Vec<_>>();
    assert_eq!(entries.first(), Some(&global), "{entries:?}");
    assert_eq!(entries.last(), Some(&managed), "{entries:?}");

    tool(&global, "yt-dlp", "2020.01.01");
    let deps = dependencies(&data, &global);
    assert_eq!(
        deps.executable("yt-dlp", &token).await.unwrap(),
        managed.join("yt-dlp")
    );
    let path = deps.child_path(&token).await.unwrap();
    assert_eq!(std::env::split_paths(&path).next(), Some(managed));

    assert!(deps.executable("ffmpeg", &token).await.is_err());
    token.cancel();
    assert!(deps.child_path(&token).await.is_err());
}
