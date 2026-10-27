mod common;
use std::process::Command;
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fetch-mix"));
    command.env("AGENT", "1");
    for key in [
        "OLLAMA_HOST",
        "OLLAMA_URL",
        "LITELLM_BASE_URL",
        "LITELLM_URL",
        "LITELLM_API_BASE",
        "LITELLM_API_KEY",
        "LITELLM_KEY",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "GEMINI_API_KEY",
        "ANTHROPIC_API_KEY",
        "OPENROUTER_API_KEY",
        "DEEPSEEK_API_KEY",
        "GROQ_API_KEY",
    ] {
        command.env_remove(key);
    }
    command
}
#[test]
fn metadata_validation_and_real_web_dry_run() {
    let output = command().arg("--version").output().unwrap();
    assert_eq!(output.stdout, b"0.1.0\n");
    for path in [
        vec!["--help"],
        vec!["ai", "--help"],
        vec!["dependencies", "install", "--help"],
        vec!["help", "youtube"],
    ] {
        let output = command().args(path).output().unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .starts_with("ALERT: Agents must read")
        );
    }
    for path in [
        vec!["ai", "inspect", "unknown"],
        vec!["dependencies", "install", "unknown"],
        vec!["--sources", "bogus", "query"],
        vec!["--llm-provider", "bogus", "query"],
        vec!["skill", "extra"],
    ] {
        let output = command().args(path).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = command().arg("upgrade").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unpublished"));
    let output = command().args(["ai", "list"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 9);
    let output = command()
        .args(["ai", "list"])
        .env("OPENAI_API_KEY", "private")
        .env("OLLAMA_MODEL", "local-model")
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(
        "ollama\tlocal-model\tOLLAMA_HOST\tNot Detected (default: http://localhost:11434)"
    ));
    assert!(text.contains("openai\tgpt-4o-mini\tOPENAI_API_KEY\tWILL BE USED"));
    let output = command()
        .args(["ai", "inspect", " Custom OpenAI-Compat "])
        .env("OPENAI_BASE_URL", "https://username:secret@example.org/v1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "provider: custom\nname: Custom OpenAI-Compat\ndefault_model: gpt-4o-mini\nenv_var: OPENAI_BASE_URL\nstatus: WILL BE USED\n"
    );
    let server =
        common::Http::new(|request| common::json(request, serde_json::json!({"ignored":true})));
    let server_html = common::Http::new(|request| {
        request
            .respond(tiny_http::Response::from_string(
                "<h1>Tracklist</h1><p>01. Alpha - One</p><p>02. Beta - Two</p>",
            ))
            .unwrap()
    });
    let directory = tempfile::tempdir_in(".tmp").unwrap();
    let output = command()
        .args([
            "--dry-run",
            "--no-cache",
            "--progress-target",
            "stdout",
            &format!("{}/set", server_html.url),
        ])
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(value["type"].is_string());
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    drop(server);
}

#[test]
fn native_dependencies_web_download_cached_youtube_and_provider_inspection() {
    let directory = tempfile::tempdir_in(".tmp").unwrap();
    let directory = directory.path().canonicalize().unwrap();
    let bin = directory.join("fixture-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fixture = common::track_fixture();
    for name in ["fetch-track", "yt-dlp", "ffmpeg"] {
        std::fs::copy(
            &fixture,
            bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        )
        .unwrap();
    }
    let run = || {
        let mut command = command();
        command
            .current_dir(&directory)
            .env("HOME", &directory)
            .env("XDG_DATA_HOME", directory.join("data"))
            .env("XDG_CACHE_HOME", directory.join("cache"))
            .env("PATH", &bin);
        command
    };
    let output = run().args(["dependencies", "verify"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 3);
    let output = run().args(["deps", "install"]).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("satisfied"));
    let output = run()
        .args(["dependencies", "install", "fetch-track"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unpublished"));
    for name in [
        "ollama",
        "litellm",
        "custom",
        "gemini",
        "openai",
        "anthropic",
        "openrouter",
        "deepseek",
        "groq",
    ] {
        let output = run()
            .args(["ai", "inspect", name])
            .env("OLLAMA_HOST", "http://localhost:11434")
            .env("OPENAI_BASE_URL", "http://localhost:4000/v1")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("provider: {name}")));
    }
    let server = common::Http::new(|request| {
        request
            .respond(tiny_http::Response::from_string(
                "<h1>Tracklist</h1><p>01. Native - Song</p>",
            ))
            .unwrap()
    });
    let output = run()
        .args([
            "--no-cache",
            "--skip-verify",
            "--skip-metadata",
            "--verbose",
            "--progress-target",
            "stdout",
            "--out-dir",
        ])
        .arg(directory.join("mix"))
        .arg(format!("{}/native-set", server.url))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "track_complete" && e["result"]["bandwidth_hz"] == 20000)
    );
    let last = events.last().unwrap();
    assert_eq!(last["summary"]["downloaded"], 1);
    assert!(
        last["summary"]["target_dir"]
            .as_str()
            .unwrap()
            .starts_with("~/")
    );
    assert!(directory.join("mix/01 - chosen.flac").exists());
    std::fs::create_dir_all(directory.join(".tmp")).unwrap();
    std::fs::write(directory.join(".tmp/yt_tracks_dQw4w9WgXcQ.json"),r#"{"videoId":"dQw4w9WgXcQ","videoTitle":"Cached mix","tracks":[{"artist":"Cached","title":"Track"}],"skippedItems":[]}"#).unwrap();
    let output = run()
        .args(["yt", "dQw4w9WgXcQ", "--dry-run"])
        .env("FETCH_MIX_DEV", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Cached"));
    for args in [vec![], vec!["youtube"], vec!["ai"], vec!["ai", "inspect"]] {
        assert!(run().args(args).output().unwrap().status.success())
    }
}

#[cfg(unix)]
#[test]
fn native_configured_release_install_ensure_update_and_self_replace() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir_in(".tmp").unwrap();
    let directory = temp.path().canonicalize().unwrap();
    let bin = directory.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["yt-dlp", "ffmpeg"] {
        std::fs::copy(common::track_fixture(), bin.join(name)).unwrap();
    }
    let package = bin.join("apt-get");
    std::fs::write(&package, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(package, std::fs::Permissions::from_mode(0o755)).unwrap();
    let track_archive = common::archive(
        "fetch-track",
        &std::fs::read(common::track_fixture()).unwrap(),
    );
    let mix_archive = common::archive("fetch-mix", b"#!/bin/sh\nprintf '0.2.0\\n'\n");
    let server = common::Http::new(move |request| {
        let path = request.url().to_owned();
        if path.ends_with("/latest") {
            let repo = if path.contains("rust-track") {
                "rust-track"
            } else {
                "rust-mix"
            };
            request
                .respond(
                    tiny_http::Response::empty(302).with_header(
                        tiny_http::Header::from_bytes(
                            "Location",
                            format!("/owner/{repo}/releases/tag/v0.2.0"),
                        )
                        .unwrap(),
                    ),
                )
                .unwrap()
        } else if path.contains("/releases/download/") {
            request
                .respond(tiny_http::Response::from_data(
                    if path.contains("rust-track") {
                        track_archive.clone()
                    } else {
                        mix_archive.clone()
                    },
                ))
                .unwrap()
        } else {
            request
                .respond(tiny_http::Response::from_string(
                    "<h1>Tracklist</h1><p>Artist - Song</p>",
                ))
                .unwrap()
        }
    });
    let run = || {
        let mut command = command();
        command
            .current_dir(&directory)
            .env("HOME", &directory)
            .env("XDG_DATA_HOME", directory.join("data"))
            .env("PATH", &bin)
            .env("FETCH_TRACK_RUST_RELEASE_BASE_URL", &server.url)
            .env("FETCH_TRACK_RUST_RELEASE_OWNER", "owner")
            .env("FETCH_TRACK_RUST_RELEASE_REPO", "rust-track");
        command
    };
    let output = run().args(["deps", "verify"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("FAIL"));
    let output = run().arg(format!("{}/set", server.url)).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsatisfied"));
    let output = run()
        .args(["--auto-install", "--no-cache", "--out-dir", "mix"])
        .arg(format!("{}/set", server.url))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("installed: fetch-track"));
    for args in [
        vec!["deps", "update", "fetch-track"],
        vec!["deps", "update"],
        vec!["deps", "install", "fetch-track"],
    ] {
        let output = run().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let copy = directory.join("fetch-mix");
    std::fs::copy(env!("CARGO_BIN_EXE_fetch-mix"), &copy).unwrap();
    let output = Command::new(&copy)
        .arg("upgrade")
        .env("AGENT", "1")
        .env("FETCH_MIX_RUST_RELEASE_BASE_URL", &server.url)
        .env("FETCH_MIX_RUST_RELEASE_OWNER", "owner")
        .env("FETCH_MIX_RUST_RELEASE_REPO", "rust-mix")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(std::fs::read(&copy).unwrap().starts_with(b"#!/bin/sh"));
    assert_eq!(
        Command::new(&copy)
            .arg("--version")
            .output()
            .unwrap()
            .stdout,
        b"0.2.0\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn pending_human_prompt_cancels_without_stdin_or_installation() {
    use tokio::io::AsyncBufReadExt;
    let directory = tempfile::tempdir_in(".tmp").unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_fetch-mix"));
    command
        .arg("http://127.0.0.1:1/set")
        .env("AGENT", "0")
        .env("HOME", directory.path())
        .env("XDG_DATA_HOME", directory.path())
        .env("PATH", directory.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let input = child.stdin.take().unwrap();
    let mut output = tokio::io::BufReader::new(child.stdout.take().unwrap());
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let mut line = String::new();
            assert!(output.read_line(&mut line).await.unwrap() > 0);
            if line.contains("[y/N]") {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        unsafe { libc::kill(child.id().unwrap() as i32, libc::SIGTERM) },
        0
    );
    let output = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    drop(input);
    assert_eq!(output.status.code(), Some(130));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("[ERROR]")
            .count(),
        1
    );
}
