mod common;
use fetch_mix_cli::{
    CancellationToken,
    cache::Cache,
    llm::{Environment, Llm, ProviderConfig, providers},
    web::{Discovery, Endpoints},
};
use serde_json::json;

#[tokio::test]
async fn search_raw_scrape_mirrors_cache_and_cancellation() {
    let server = common::Http::new(|request| {
        let url = request.url().to_owned();
        if url.starts_with("/w/api.php") {
            assert!(url.contains("srsearch="));
            common::json(
                request,
                json!({"query":{"search":[{"title":"Bicep - Essential Mix"}]}}),
            );
        } else if url.starts_with("/mirror") {
            common::json(
                request,
                json!([{"link":"https://example.org/set","title":{"rendered":"Polo &#038; Pan"}}]),
            );
        } else if url.contains("action=raw") {
            request
                .respond(tiny_http::Response::from_string(
                    "== Tracklist ==\n# [00:31] Bicep - Glue",
                ))
                .unwrap();
        } else if url.starts_with("/html") {
            request
                .respond(tiny_http::Response::from_string(
                    "<h2>Tracklist</h2><ul><li>A - B</li></ul>",
                ))
                .unwrap();
        } else {
            request.respond(tiny_http::Response::empty(500)).unwrap();
        }
    });
    let endpoints = Endpoints {
        mixes_db: format!("{}/", server.url),
        mirrors: vec![
            format!("{}/bad", server.url),
            format!("{}/mirror", server.url),
        ],
    };
    let discovery = Discovery::new(endpoints).unwrap();
    let token = CancellationToken::new();
    let found = discovery
        .search(
            "https://www.1001tracklists.com/tracklist/a/bicep-essential-mix.HTML",
            &token,
        )
        .await
        .unwrap();
    assert_eq!(found[0].title, "Bicep - Essential Mix");
    let mirrors = discovery.mirrors("Polo & Pan", &token).await.unwrap();
    assert_eq!(mirrors[0].title, "Polo & Pan");
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let cache = Cache::new(dir.path().join("cache"));
    let content = discovery
        .scrape(&found[0].url, Some(&cache), &token)
        .await
        .unwrap();
    assert!(content.starts_with("== Tracklist"));
    let html = discovery
        .scrape(&format!("{}/html", server.url), None, &token)
        .await
        .unwrap();
    assert!(html.contains("A - B"));
    assert!(
        discovery
            .scrape(&format!("{}/bad", server.url), None, &token)
            .await
            .is_err()
    );
    drop(server);
    assert_eq!(
        discovery
            .scrape(&found[0].url, Some(&cache), &token)
            .await
            .unwrap(),
        content
    );
    token.cancel();
    assert!(discovery.search("query", &token).await.is_err());
}

#[tokio::test]
async fn native_structured_ai_and_provider_selection() {
    let server = common::Http::new(|mut request| {
        assert_eq!(request.url(), "/v1/chat/completions");
        let mut text = String::new();
        request.as_reader().read_to_string(&mut text).unwrap();
        let body: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["model"], "fixture-model");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("JSON")
        );
        common::json(
            request,
            json!({"id":"fixture","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":json!({"found":true,"commentId":"c1","tracks":[{"artist":" A ","title":" B ","timestamp":"00:00"},{"artist":"","title":"Invalid"}],"skippedItems":[]}).to_string()},"finish_reason":"stop"}]}),
        );
    });
    let env: Environment = [
        ("OPENAI_BASE_URL".into(), format!("{}/v1/", server.url)),
        ("OPENAI_API_KEY".into(), "fixture-key".into()),
    ]
    .into();
    let config = ProviderConfig::resolve("custom", "fixture-model", &env).unwrap();
    let llm = Llm::new(config).await.unwrap();
    let extraction = llm
        .extract_content(
            "untrusted - content",
            "set title",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(extraction.tracks.len(), 1);
    assert_eq!(extraction.tracks[0].raw_string, "A - B");
    assert_eq!(
        ProviderConfig::resolve("auto", "", &env)
            .unwrap()
            .provider
            .id,
        "openai"
    );
    assert!(ProviderConfig::resolve("unknown", "", &env).is_err());
    assert!(ProviderConfig::resolve("auto", "", &Environment::new()).is_err());
    assert!(ProviderConfig::resolve("custom", "", &Environment::new()).is_err());
    for provider in providers() {
        let env: Environment = provider
            .environment
            .iter()
            .map(|key| {
                (
                    (*key).into(),
                    if key.contains("URL") || key.contains("HOST") || key.contains("BASE") {
                        "http://localhost:4000".into()
                    } else {
                        "fixture".into()
                    },
                )
            })
            .collect();
        let config = ProviderConfig::resolve(provider.id, "", &env).unwrap();
        assert_eq!(config.provider.id, provider.id);
    }
    let env: Environment = [(
        "LITELLM_API_BASE".into(),
        "http://localhost:4000/v1/".into(),
    )]
    .into();
    assert_eq!(
        ProviderConfig::resolve("auto", "", &env)
            .unwrap()
            .provider
            .id,
        "litellm"
    );
    let env: Environment = [
        ("OLLAMA_URL".into(), "http://localhost:11434".into()),
        ("OLLAMA_MODEL".into(), " local ".into()),
        ("LITELLM_KEY".into(), "secret".into()),
        ("OPENAI_MODEL".into(), "unsupported".into()),
    ]
    .into();
    let states = fetch_mix_cli::llm::statuses(&env);
    assert_eq!(states[0].status, "WILL BE USED");
    assert_eq!(states[0].model, "local");
    assert_eq!(states[1].status, "Detected");
    assert_eq!(states[2].status, "Not Detected");
    assert_eq!(
        ProviderConfig::resolve("openai", "", &env).unwrap().model,
        "gpt-4o-mini"
    );
    assert_eq!(
        ProviderConfig::resolve("auto", "", &env).unwrap().model,
        "local"
    );
    assert!(
        ProviderConfig::resolve(
            "ollama",
            "",
            &[("OLLAMA_URL".into(), "file:///private".into())].into()
        )
        .is_err()
    );
}

#[tokio::test]
async fn native_pipeline_search_selects_first_and_reports_empty_results() {
    use cli_command_kit::Mode;
    use fetch_mix_cli::cli;
    let server = common::Http::new(|request| {
        if request.url().contains("srsearch=missing") {
            common::json(request, json!({"query":{"search":[]}}));
        } else if request.url().starts_with("/w/api.php") {
            common::json(
                request,
                json!({"query":{"search":[{"title":"First set"},{"title":"Second set"}]}}),
            );
        } else if request.url().contains("action=raw") {
            request
                .respond(tiny_http::Response::from_string(
                    "== Tracklist ==\n# Alpha - One",
                ))
                .unwrap();
        } else {
            common::json(request, json!([]));
        }
    });
    let endpoints = Endpoints {
        mixes_db: format!("{}/", server.url),
        mirrors: vec![format!("{}/mirror", server.url)],
    };
    let matches = cli::command(Mode::Agent)
        .try_get_matches_from(["fetch-mix", "--dry-run", "--no-cache", "query"])
        .unwrap();
    cli::execute(
        &matches,
        Mode::Agent,
        &Environment::new(),
        &CancellationToken::new(),
        endpoints.clone(),
    )
    .await
    .unwrap();
    let matches = cli::command(Mode::Agent)
        .try_get_matches_from(["fetch-mix", "--dry-run", "--no-cache", "missing"])
        .unwrap();
    assert!(
        cli::execute(
            &matches,
            Mode::Agent,
            &Environment::new(),
            &CancellationToken::new(),
            endpoints
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("no matching")
    );
}
