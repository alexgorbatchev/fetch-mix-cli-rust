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
    let dir = tempfile::tempdir_in(".tmp").unwrap();
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
