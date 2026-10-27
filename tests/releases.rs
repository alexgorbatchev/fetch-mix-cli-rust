mod common;
use fetch_mix_cli::{CancellationToken, llm::Environment, releases};
use std::sync::Arc;
#[tokio::test]
async fn explicit_release_source_installs_native_archive_and_updates_atomically() {
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut archive = tar::Builder::new(&mut gzip);
        let bytes = b"#!/bin/sh\nprintf '0.2.0\\n'\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "fetch-mix", &bytes[..])
            .unwrap();
        archive.finish().unwrap();
    }
    let archive = Arc::new(gzip.finish().unwrap());
    let server = common::Http::new(move |request| {
        if request.url().ends_with("/latest") {
            request
                .respond(
                    tiny_http::Response::empty(302).with_header(
                        tiny_http::Header::from_bytes(
                            "Location",
                            "/owner/rust-mix/releases/tag/v0.2.0",
                        )
                        .unwrap(),
                    ),
                )
                .unwrap()
        } else {
            request
                .respond(tiny_http::Response::from_data(archive.as_ref().clone()))
                .unwrap()
        }
    });
    let env = Environment::from([
        ("FETCH_MIX_RUST_RELEASE_BASE_URL".into(), server.url.clone()),
        ("FETCH_MIX_RUST_RELEASE_OWNER".into(), "owner".into()),
        ("FETCH_MIX_RUST_RELEASE_REPO".into(), "rust-mix".into()),
    ]);
    let source = releases::from_environment("FETCH_MIX", &env)
        .unwrap()
        .unwrap();
    let token = CancellationToken::new();
    let target = dir.path().join("fetch-mix");
    std::fs::write(&target, "old").unwrap();
    let updater = cli_deps::Updater {
        channel: Some(source.clone()),
    };
    let update = updater.update_to("0.1.0", &target, &token).await.unwrap();
    assert_eq!(update, cli_deps::Update::Installed("0.2.0".into()));
    assert!(std::fs::read_to_string(&target).unwrap().contains("0.2.0"));
    let update = updater.update_to("0.2.0", &target, &token).await.unwrap();
    assert_eq!(update, cli_deps::Update::Current("0.2.0".into()));
    assert_eq!(
        source
            .client
            .install_release(
                &source.owner,
                &source.repository,
                "fetch-mix",
                dir.path(),
                &token
            )
            .await
            .unwrap(),
        "v0.2.0"
    );
    token.cancel();
    assert!(matches!(
        source
            .client
            .install_release(
                &source.owner,
                &source.repository,
                "fetch-mix",
                dir.path(),
                &token
            )
            .await,
        Err(cli_deps::Error::Cancelled)
    ));
    assert!(
        releases::from_environment("FETCH_MIX", &Environment::new())
            .unwrap()
            .is_none()
    );
    let mut bad = env.clone();
    bad.remove("FETCH_MIX_RUST_RELEASE_REPO");
    assert!(releases::from_environment("FETCH_MIX", &bad).is_err());
    bad = env.clone();
    bad.insert(
        "FETCH_MIX_RUST_RELEASE_BASE_URL".into(),
        "http://example.org".into(),
    );
    assert!(releases::from_environment("FETCH_MIX", &bad).is_err());
    bad = env;
    bad.insert("FETCH_MIX_RUST_RELEASE_OWNER".into(), "../bad".into());
    assert!(releases::from_environment("FETCH_MIX", &bad).is_err());
}
