use fetch_mix_cli::{
    CancellationToken,
    progress::{Event, Reporter, Server, Target},
};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn shared_reporter_delivers_mix_schema_and_preserves_forwarded_timestamps() {
    use tokio::io::AsyncReadExt;
    let (writer, mut reader) = tokio::io::duplex(4096);
    let reporter = Reporter::writer(writer);
    let token = CancellationToken::new();
    let mut event = Event::new("complete", "download");
    event.result = Some(serde_json::json!({"path":"native.flac"}));
    event.timestamp = "2026-10-14T16:34:00Z".into();
    reporter.emit(event.stamped(), &token).await.unwrap();
    drop(reporter);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["result"]["path"], "native.flac");
    assert_eq!(value["timestamp"], "2026-10-14T16:34:00Z");
    let event = Event::default().stamped();
    assert!(chrono::DateTime::parse_from_rfc3339(&event.timestamp).is_ok());
}

#[tokio::test]
async fn sockets_forward_owned_fields_and_close_with_open_peer() {
    for tcp in [false, true] {
        let dir = tempfile::tempdir_in(".tmp").unwrap();
        let token = CancellationToken::new();
        let mut server = Server::start(dir.path(), tcp, &token).await.unwrap();
        let reporter = Reporter::connect(&Target::parse(&server.target).unwrap(), &token)
            .await
            .unwrap();
        let mut event = Event::new("complete", "download");
        event.result = Some(
            serde_json::json!({"path":"song.flac","suggested_gain_db":0.0,"extra":"preserved"}),
        );
        reporter.emit(event.stamped(), &token).await.unwrap();
        let got = server.next().await.unwrap().unwrap();
        assert_eq!(got.result.unwrap()["extra"], "preserved");
        assert!(!got.timestamp.is_empty());
        tokio::time::timeout(std::time::Duration::from_secs(2), server.finish())
            .await
            .unwrap()
            .unwrap();
        drop(reporter);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn finish_drains_connection_queued_before_server_task_runs() {
    use std::io::Write;
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let token = CancellationToken::new();
    for tcp in [false, true] {
        let server = Server::start(dir.path(), tcp, &token).await.unwrap();
        // Native connect/write completes in the kernel without yielding this
        // thread to the server's accept task, as a fast child can do at exit.
        let mut peer: Box<dyn Write> = if let Some(address) = server.target.strip_prefix("tcp://") {
            Box::new(std::net::TcpStream::connect(address).unwrap())
        } else {
            #[cfg(unix)]
            {
                Box::new(
                    std::os::unix::net::UnixStream::connect(
                        server.target.trim_start_matches("unix://"),
                    )
                    .unwrap(),
                )
            }
            #[cfg(not(unix))]
            {
                unreachable!("non-Unix uses TCP")
            }
        };
        peer.write_all(b"{\"type\":\"complete\",\"result\":{\"bandwidth_hz\":20000}}\n")
            .unwrap();
        drop(peer);
        let events = server.finish().await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].result.as_ref().unwrap()["bandwidth_hz"], 20000);
    }
}

#[tokio::test]
async fn fragmented_events_and_cancelled_reporter() {
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let token = CancellationToken::new();
    let mut server = Server::start(dir.path(), true, &token).await.unwrap();
    let mut peer = tokio::net::TcpStream::connect(server.target.trim_start_matches("tcp://"))
        .await
        .unwrap();
    peer.write_all(b"\n{bad}\n{\"type\":\"progress\",")
        .await
        .unwrap();
    peer.write_all(b"\"message\":\"hello\"}\n").await.unwrap();
    assert_eq!(server.next().await.unwrap().unwrap().message, "hello");
    peer.shutdown().await.unwrap();
    server.finish().await.unwrap();
    token.cancel();
    assert!(
        Reporter::connect(&Target::parse("tcp://127.0.0.1:1").unwrap(), &token)
            .await
            .is_err()
    );
    for input in ["", "ftp://host", "fd://bad", "no-target"] {
        assert!(Target::parse(input).is_err());
    }
    assert_eq!(
        Target::parse("unix://.tmp/song.sock").unwrap(),
        Target::Unix(".tmp/song.sock".into())
    );
    assert_eq!(
        Target::parse("./song.sock").unwrap(),
        Target::Unix("./song.sock".into())
    );
    assert_eq!(
        Target::parse("127.0.0.1:2").unwrap(),
        Target::Tcp("127.0.0.1:2".into())
    );
}

#[tokio::test]
async fn oversized_frame_is_bounded_and_fd_ownership_is_preserved() {
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let token = CancellationToken::new();
    let mut server = Server::start(dir.path(), true, &token).await.unwrap();
    let mut peer = tokio::net::TcpStream::connect(server.target.trim_start_matches("tcp://"))
        .await
        .unwrap();
    peer.write_all(&vec![b'x'; 1024 * 1024 + 10]).await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), server.next())
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    server.finish().await.unwrap();
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let file = tempfile::tempfile_in(dir.path()).unwrap();
        let reporter = Reporter::connect(&Target::Fd(file.as_raw_fd()), &token)
            .await
            .unwrap();
        reporter
            .emit(Event::new("progress", "test"), &token)
            .await
            .unwrap();
        drop(reporter);
        assert!(file.metadata().unwrap().len() > 0);
        assert!(
            Reporter::connect(&Target::Fd(2147483647), &token)
                .await
                .is_err()
        );
    }
    let reporter = Reporter::connect(&Target::Stderr, &token).await.unwrap();
    token.cancel();
    assert!(
        reporter
            .emit(Event::new("error", "test"), &token)
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn blocked_fd_cancel_does_not_retain_runtime_worker() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "blocked_fd_helper", "--ignored"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("cancelled descriptor retained a blocking runtime worker");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[cfg(unix)]
#[test]
#[ignore = "invoked with a bounded parent process"]
fn blocked_fd_helper() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let mut raw = [-1; 2];
    assert_eq!(unsafe { libc::pipe(raw.as_mut_ptr()) }, 0);
    let read = unsafe { OwnedFd::from_raw_fd(raw[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(raw[1]) };
    let flags = unsafe { libc::fcntl(write.as_raw_fd(), libc::F_GETFL) };
    assert_eq!(
        unsafe { libc::fcntl(write.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    let buffer = [b'x'; 4096];
    while unsafe { libc::write(write.as_raw_fd(), buffer.as_ptr().cast(), buffer.len()) } > 0 {}
    assert_eq!(
        std::io::Error::last_os_error().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        unsafe { libc::fcntl(write.as_raw_fd(), libc::F_SETFL, flags) },
        0
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let token = CancellationToken::new();
        let reporter = Reporter::connect(&Target::Fd(write.as_raw_fd()), &token)
            .await
            .unwrap();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            cancel.cancel();
        });
        assert!(matches!(
            reporter
                .emit(Event::new("progress", "blocked"), &token)
                .await,
            Err(cli_event_stream::Error::Cancelled)
        ));
        drop(reporter);
    });
    drop(runtime);
    assert_eq!(
        unsafe { libc::fcntl(write.as_raw_fd(), libc::F_GETFL) },
        flags
    );
    drop((read, write));
}
