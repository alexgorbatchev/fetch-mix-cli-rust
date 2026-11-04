mod common;
use fetch_mix_cli::{
    CancellationToken,
    cache::Cache,
    llm::{Environment, Llm, ProviderConfig},
    youtube::{self, Comment},
};
use serde_json::json;
#[tokio::test]
async fn comments_native_process_ai_and_cache() {
    let dir = common::scratch();
    let script = dir.path().join("yt-dlp");
    let comments = json!({"title":"Live DJ Set","comments":[{"id":"c1","author":"Fan","text":"01. A - B\n02. C - D\n03. E - F","like_count":100}]});
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", comments),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let server = common::Http::new(|request| {
        common::json(
            request,
            json!({"choices":[{"index":0,"message":{"role":"assistant","content":"{\"found\":true,\"commentId\":\"c1\",\"tracks\":[{\"artist\":\"A\",\"title\":\"B\"}],\"skippedItems\":[]}"},"finish_reason":"stop"}]}),
        )
    });
    let env: Environment = [("OPENAI_BASE_URL".into(), format!("{}/v1", server.url))].into();
    let llm = Llm::new(ProviderConfig::resolve("custom", "", &env).unwrap())
        .await
        .unwrap();
    let cache = Cache::new(dir.path().join("cache"));
    let token = CancellationToken::new();
    let result = youtube::process(
        "V19eCd8Odo8",
        &script,
        None,
        Some(&cache),
        Some(&llm),
        &token,
    )
    .await
    .unwrap();
    assert_eq!(result.video_title, "Live DJ Set");
    assert_eq!(result.tracks[0].raw_string, "A - B");
    std::fs::remove_file(script.clone()).unwrap();
    assert_eq!(
        youtube::process("V19eCd8Odo8", &script, None, Some(&cache), None, &token)
            .await
            .unwrap()
            .tracks,
        result.tracks
    );
    assert!(
        youtube::process("invalid", &script, None, None, None, &token)
            .await
            .is_err()
    );
    assert!(
        youtube::process("11111111111", &script, None, None, None, &token)
            .await
            .is_err()
    );
    for (input, id) in [
        ("https://youtu.be/V19eCd8Odo8?t=2", "V19eCd8Odo8"),
        ("https://youtube.com/shorts/V19eCd8Odo8", "V19eCd8Odo8"),
        ("https://www.youtube.com/watch?v=V19eCd8Odo8", "V19eCd8Odo8"),
        ("https://youtube.com.evil.invalid/watch?v=V19eCd8Odo8", ""),
    ] {
        assert_eq!(youtube::video_id(input).unwrap_or_default(), id);
    }
    let mut comments: Vec<Comment> = serde_json::from_value(comments["comments"].clone()).unwrap();
    comments.push(Comment {
        id: "higher".into(),
        like_count: 200,
        ..comments[0].clone()
    });
    assert_eq!(youtube::filter_comments(&comments)[0].id, "higher");
}

#[cfg(unix)]
fn fake_yt_dlp(directory: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = directory.join("yt-dlp");
    std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
async fn failure(script: &std::path::Path, token: &CancellationToken) -> fetch_mix_cli::Error {
    match youtube::process("V19eCd8Odo8", script, None, None, None, token).await {
        Ok(_) => panic!("extraction succeeded"),
        Err(error) => error,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn extraction_reports_yt_dlp_failures_and_unusable_comments() {
    let dir = common::scratch();
    let token = CancellationToken::new();
    let script = fake_yt_dlp(
        dir.path(),
        "printf 'WARNING: noise\\nERROR: [youtube] V19eCd8Odo8: Video unavailable\\n' >&2; exit 1",
    );
    let error = failure(&script, &token).await;
    assert_eq!(
        error.to_string(),
        "yt-dlp comment extraction failed: ERROR: [youtube] V19eCd8Odo8: Video unavailable"
    );
    let script = fake_yt_dlp(
        dir.path(),
        r#"printf '%s\n' '{"title":"Set","comments":[{"id":"c1","text":"great set"}]}'"#,
    );
    let error = failure(&script, &token).await;
    assert!(error.to_string().contains("no comments found"), "{error}");
    let tracklist =
        json!({"title":"Set","comments":[{"id":"c1","text":"01. A - B\n02. C - D\n03. E - F"}]});
    let script = fake_yt_dlp(dir.path(), &format!("printf '%s\\n' '{tracklist}'"));
    let error = failure(&script, &token).await;
    assert!(
        error.to_string().contains("configured AI provider"),
        "{error}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_extraction_terminates_yt_dlp_with_sigterm() {
    let dir = common::scratch();
    let started = dir.path().join("started");
    let log = dir.path().join("log");
    let script = fake_yt_dlp(
        dir.path(),
        &format!(
            "trap 'echo term > \"{}\"; exit 0' TERM\nprintf %s $$ > \"{}\"\nwhile :; do sleep 1; done",
            log.display(),
            started.display()
        ),
    );
    let token = CancellationToken::new();
    let operation = youtube::process("V19eCd8Odo8", &script, None, None, None, &token);
    tokio::pin!(operation);
    tokio::select! {
        result = &mut operation => panic!("premature exit: {:?}", result.err()),
        _ = async {
            while std::fs::read_to_string(&started).map_or(true, |s| s.is_empty()) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        } => {}
    }
    token.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), &mut operation)
        .await
        .expect("cancellation finished");
    assert!(matches!(result, Err(fetch_mix_cli::Error::Cancelled)));
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "term");
    let pid: libc::pid_t = std::fs::read_to_string(&started).unwrap().parse().unwrap();
    assert_ne!(
        unsafe { libc::kill(pid, 0) },
        0,
        "yt-dlp outlived cancellation"
    );
}
