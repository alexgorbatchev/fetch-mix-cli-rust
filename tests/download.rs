use cli_command_kit::Mode;
mod common;
use fetch_mix_cli::{
    CancellationToken,
    download::{DownloadOptions, download},
    manifest::Manifest,
    model::Track,
    ui::Ui,
};

#[tokio::test]
async fn dry_run_has_no_files_and_resume_validates_identity() {
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let target = dir.path().join("mix");
    let options = DownloadOptions {
        title: "..".into(),
        tracks: vec![Track::new("A", "B", "00:01")],
        output: Some(target.clone()),
        dry_run: true,
        delay: std::time::Duration::ZERO,
        ..Default::default()
    };
    let ui = Ui::new(Mode::Agent, std::io::sink());
    download(&options, &ui, &CancellationToken::new())
        .await
        .unwrap();
    assert!(!target.exists());
    std::fs::create_dir_all(&target).unwrap();
    let manifest = Manifest::load_or_create(&target, "Mix", &options.tracks, &[]).unwrap();
    manifest.save(&target).unwrap();
    assert!(
        Manifest::load_or_create(&target, "Mix", &[Track::new("Different", "Song", "")], &[])
            .is_err()
    );
    assert!(Manifest::load_or_create(&target, "Other", &options.tracks, &[]).is_err());
    assert_eq!(fetch_mix_cli::download::sanitize(".."), "Downloaded DJ Set");
}

#[tokio::test]
async fn actual_progress_stream_preserves_result_metrics_and_context() {
    use fetch_mix_cli::progress::{Reporter, Server, Target};
    let directory = tempfile::tempdir_in(".tmp").unwrap();
    let token = CancellationToken::new();
    let mut server = Server::start(directory.path(), true, &token).await.unwrap();
    let reporter = Reporter::connect(&Target::parse(&server.target).unwrap(), &token)
        .await
        .unwrap();
    let options = DownloadOptions {
        title: "Native mix".into(),
        tracks: vec![Track::new("Artist", "Title", "1:23")],
        output: Some(directory.path().join("output")),
        reporter: Some(reporter),
        executable: common::track_fixture(),
        force_tcp: true,
        skip_metadata: true,
        skip_verify: true,
        verbose: true,
        delay: std::time::Duration::ZERO,
        ..Default::default()
    };
    let ui = Ui::new(Mode::Human, std::io::sink());
    let mut events = Vec::new();
    {
        let operation = download(&options, &ui, &token);
        tokio::pin!(operation);
        loop {
            tokio::select! {result=&mut operation=>{result.unwrap();break},event=server.next()=>{events.push(event.unwrap().unwrap())}}
        }
    }
    drop(options);
    events.extend(server.finish().await.unwrap());
    let completed = events
        .iter()
        .find(|event| event.kind == "track_complete")
        .unwrap();
    assert_eq!(completed.track.as_ref().unwrap()["bandwidth_hz"], 20000);
    assert_eq!(completed.track.as_ref().unwrap()["index"], 1);
    assert_eq!(
        completed.result.as_ref().unwrap()["suggested_gain_db"],
        -2.5
    );
    assert!(events.iter().any(|e| e.extra.get("future_field").is_some()));
    let manifest = Manifest::load_or_create(
        &directory.path().join("output"),
        "Native mix",
        &[Track::new("Artist", "Title", "1:23")],
        &[],
    )
    .unwrap();
    assert_eq!(
        manifest.tracks[0].source_url,
        "https://example.invalid/audio"
    );
    assert_eq!(manifest.tracks[0].actual_file, "01 - chosen.flac");
}

#[cfg(unix)]
#[tokio::test]
async fn child_artifact_numbering_playlist_resume_and_partial_failure() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let script = dir.path().join("fetch-track");
    std::fs::write(&script,"#!/bin/sh\ncase \"$1\" in *Fail*) printf 'ERROR: fixture failure\\n' >&2; exit 1;; esac\nprintf audio > \"$3/Artist - Song.flac\"\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let target = dir.path().join("mix");
    let mut options = DownloadOptions {
        title: "Mix".into(),
        tracks: vec![Track::new("A", "Song", ""), Track::new("Fail", "Song", "")],
        output: Some(target.clone()),
        executable: script.clone(),
        force_tcp: true,
        delay: std::time::Duration::ZERO,
        ..Default::default()
    };
    let ui = Ui::new(Mode::Human, std::io::sink());
    let error = download(&options, &ui, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        fetch_mix_cli::Error::Partial { failed: 1, .. }
    ));
    let manifest = Manifest::load_or_create(&target, "Mix", &options.tracks, &[]).unwrap();
    assert_eq!(manifest.tracks[0].actual_file, "01 - Artist - Song.flac");
    assert_eq!(manifest.tracks[1].status, "failed");
    assert!(manifest.tracks[1].error.contains("fixture failure"));
    let playlist = std::fs::read_to_string(target.join("playlist.m3u")).unwrap();
    assert!(playlist.contains("01 - Artist - Song.flac"));
    options.tracks[1] = Track::new("Fail", "Song", "");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let error = download(&options, &ui, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        fetch_mix_cli::Error::Partial { failed: 1, .. }
    ));
    std::fs::write(&script, "#!/bin/sh\nprintf audio > \"$3/Second.m4a\"\n").unwrap();
    download(&options, &ui, &CancellationToken::new())
        .await
        .unwrap();
    let manifest = Manifest::load_or_create(&target, "Mix", &options.tracks, &[]).unwrap();
    assert_eq!(manifest.tracks[1].actual_file, "02 - Second.m4a");
    std::fs::remove_file(script).unwrap();
    download(&options, &ui, &CancellationToken::new())
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn completion_rejects_outside_empty_and_overwrite_artifacts() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir_in(".tmp").unwrap();
    let fixture = common::track_fixture();
    let fixture = format!("'{}'", fixture.to_str().unwrap().replace('\'', "'\\''"));
    let ui = Ui::new(Mode::Agent, std::io::sink());
    for mode in ["outside", "empty", "fail", "collision"] {
        let target = directory.path().join(mode);
        std::fs::create_dir(&target).unwrap();
        let script = directory.path().join(format!("child-{mode}"));
        std::fs::write(
            &script,
            format!("#!/bin/sh\nexport FETCH_MIX_FIXTURE_MODE={mode}\nexec {fixture} \"$@\"\n"),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        if mode == "collision" {
            std::fs::write(target.join("01 - chosen.flac"), b"original file").unwrap();
        }
        let options = DownloadOptions {
            title: "Guarded mix".into(),
            tracks: vec![Track::new("Artist", "Song", "")],
            output: Some(target.clone()),
            executable: script,
            force_tcp: true,
            delay: std::time::Duration::ZERO,
            ..Default::default()
        };
        assert!(matches!(
            download(&options, &ui, &CancellationToken::new()).await,
            Err(fetch_mix_cli::Error::Partial { failed: 1, .. })
        ));
        let manifest =
            Manifest::load_or_create(&target, "Guarded mix", &options.tracks, &[]).unwrap();
        assert_eq!(manifest.tracks[0].status, "failed");
        assert!(manifest.tracks[0].actual_file.is_empty());
        match mode {
            "outside" => assert!(manifest.tracks[0].error.contains("outside")),
            "empty" => assert!(manifest.tracks[0].error.contains("nonempty regular")),
            "fail" => assert!(manifest.tracks[0].error.contains("native fixture failed")),
            "collision" => assert_eq!(
                std::fs::read(target.join("01 - chosen.flac")).unwrap(),
                b"original file"
            ),
            _ => unreachable!(),
        }
    }
}
