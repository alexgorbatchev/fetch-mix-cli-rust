#![cfg(unix)]
use fetch_mix_cli::{CancellationToken, Error, process};
use std::{path::Path, time::Duration};
#[tokio::test]
async fn native_pipe_output_streaming_cancellation_and_process_family_cleanup() {
    let token = CancellationToken::new();
    let mut seen = Vec::new();
    let output = process::capture_with(
        Path::new("/bin/sh"),
        &["-c".into(), "printf alpha; printf beta >&2; exit 7".into()],
        None,
        &token,
        |bytes, stderr| {
            seen.push((bytes.to_vec(), stderr));
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"alpha");
    assert_eq!(output.stderr, b"beta");
    assert!(seen.iter().any(|(_, stderr)| *stderr));
    let directory = tempfile::tempdir_in(".tmp").unwrap();
    let leader = directory.path().join("leader");
    let member = directory.path().join("member");
    let script = format!(
        "printf '%s' $$ > '{}'; sleep 60 & printf '%s' $! > '{}'; wait",
        leader.display(),
        member.display()
    );
    let arguments = vec!["-c".into(), script];
    let operation = process::capture(Path::new("/bin/sh"), &arguments, None, &token);
    tokio::pin!(operation);
    tokio::select! {result=&mut operation=>panic!("premature exit: {result:?}"),_=async{while !member.exists(){tokio::time::sleep(Duration::from_millis(5)).await}}=>{}}
    token.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), &mut operation)
            .await
            .unwrap(),
        Err(Error::Cancelled)
    ));
    let pid: libc::pid_t = std::fs::read_to_string(leader).unwrap().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    let pid: libc::pid_t = std::fs::read_to_string(member).unwrap().parse().unwrap();
    #[cfg(target_os = "linux")]
    assert!(
        std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| stat
            .split(')')
            .nth(1)
            .is_some_and(|s| s.trim_start().starts_with('Z')))
    );
    assert!(
        process::capture(Path::new("/bin/sh"), &[], None, &token)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn output_failure_kills_child_and_success_preserves_bytes() {
    let token = CancellationToken::new();
    let result = process::capture_with(
        Path::new("/bin/sh"),
        &["-c".into(), "printf ready; sleep 60".into()],
        None,
        &token,
        |_, _| Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed display").into()),
    )
    .await;
    assert!(matches!(result, Err(Error::Io(_))));
    assert!(
        process::capture(Path::new("/nonexistent-mix-process"), &[], None, &token)
            .await
            .is_err()
    );
}
