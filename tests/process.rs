#![cfg(unix)]
mod common;
use fetch_mix_cli::{CancellationToken, Error, process};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const GRACE: Duration = Duration::from_secs(5);

/// A shell script whose leader and background member record their process IDs
/// and append the signals they trap to a log.
struct Family {
    _directory: tempfile::TempDir,
    leader: PathBuf,
    member: PathBuf,
    log: PathBuf,
}

impl Family {
    /// `leader_trap` and `member_trap` are shell `trap` actions for SIGTERM.
    fn new(leader_trap: &str, member_trap: &str) -> (Self, Vec<String>) {
        let directory = common::scratch();
        let leader = directory.path().join("leader");
        let member = directory.path().join("member");
        let log = directory.path().join("log");
        let member_trap = member_trap.replace("LOG", &log.display().to_string());
        let leader_trap = leader_trap.replace("LOG", &log.display().to_string());
        let script = format!(
            "trap '{leader_trap}' TERM
sh -c 'trap \"{member_trap}\" TERM; printf %s $$ > \"{member}.tmp\"; mv \"{member}.tmp\" \"{member}\"; while :; do sleep 1; done' &
printf %s $$ > '{leader}'
while :; do sleep 1; done",
            member = member.display(),
            leader = leader.display(),
        );
        let family = Self {
            _directory: directory,
            leader,
            member,
            log,
        };
        (family, vec!["-c".into(), script])
    }

    async fn started(&self) {
        while !self.member.exists() || !self.leader.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn ids(&self) -> Vec<libc::pid_t> {
        [&self.leader, &self.member]
            .into_iter()
            .filter_map(|path| std::fs::read_to_string(path).ok()?.parse().ok())
            .collect()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Waits up to `limit` for both processes to exit.
    async fn exited_within(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            if self.ids().into_iter().all(gone) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for Family {
    /// Kills whatever a failed assertion left running. A passing test has
    /// already verified both processes exited, and their IDs may be reused.
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        for id in self.ids() {
            unsafe {
                libc::kill(-id, libc::SIGKILL);
                libc::kill(id, libc::SIGKILL);
            }
        }
    }
}

/// True when the process no longer exists or is a zombie awaiting its parent.
fn gone(id: libc::pid_t) -> bool {
    if unsafe { libc::kill(id, 0) } != 0 {
        return true;
    }
    std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &id.to_string()])
        .output()
        .is_ok_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .starts_with('Z')
        })
}

fn group_exists(id: libc::pid_t) -> bool {
    unsafe { libc::kill(-id, 0) == 0 }
}

async fn capture(
    arguments: &[String],
    grace: Duration,
    token: &CancellationToken,
) -> fetch_mix_cli::Result<std::process::Output> {
    process::capture_with(
        Path::new("/bin/sh"),
        arguments,
        None,
        grace,
        token,
        |_, _| Ok(()),
    )
    .await
}

#[tokio::test]
async fn streams_both_pipes_and_returns_the_exit_status() {
    let token = CancellationToken::new();
    let mut seen = Vec::new();
    let output = process::capture_with(
        Path::new("/bin/sh"),
        &["-c".into(), "printf alpha; printf beta >&2; exit 7".into()],
        None,
        GRACE,
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
    assert!(seen.contains(&(b"alpha".to_vec(), false)));
    assert!(seen.contains(&(b"beta".to_vec(), true)));
    assert!(matches!(
        capture(&[], GRACE, &token).await,
        Ok(output) if output.status.success()
    ));
    token.cancel();
    assert!(matches!(
        capture(&[], GRACE, &token).await,
        Err(Error::Cancelled)
    ));
    assert!(
        process::capture_with(
            Path::new("/nonexistent-mix-process"),
            &[],
            None,
            GRACE,
            &CancellationToken::new(),
            |_, _| Ok(()),
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn cancellation_sends_sigterm_and_returns_once_the_group_exits() {
    let (family, arguments) = Family::new(
        "echo leaving >&2; echo leader >> LOG; exit 0",
        "echo leaving >&2; echo member >> LOG; exit 0",
    );
    let token = CancellationToken::new();
    let operation = capture(&arguments, Duration::from_secs(30), &token);
    tokio::pin!(operation);
    tokio::select! {
        result = &mut operation => panic!("premature exit: {result:?}"),
        _ = family.started() => {}
    }
    let leader = family.ids()[0];
    let started = Instant::now();
    token.cancel();
    let result = tokio::time::timeout(Duration::from_secs(10), &mut operation)
        .await
        .expect("cancellation finished");
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a group that exits on SIGTERM must not wait for the grace period"
    );
    let log = family.log();
    assert!(log.contains("leader") && log.contains("member"), "{log}");
    assert!(
        !group_exists(leader),
        "cancellation returned before the group exited"
    );
    assert!(family.exited_within(Duration::ZERO).await);
}

#[tokio::test]
async fn cancellation_kills_a_group_that_ignores_sigterm_after_the_grace_period() {
    // The leader survives SIGTERM after logging it; the member ignores it.
    let (family, arguments) = Family::new("echo leader >> LOG", "");
    let grace = Duration::from_millis(400);
    let token = CancellationToken::new();
    let operation = capture(&arguments, grace, &token);
    tokio::pin!(operation);
    tokio::select! {
        result = &mut operation => panic!("premature exit: {result:?}"),
        _ = family.started() => {}
    }
    let started = Instant::now();
    token.cancel();
    let result = tokio::time::timeout(Duration::from_secs(10), &mut operation)
        .await
        .expect("cancellation finished");
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(
        started.elapsed() >= grace,
        "SIGKILL arrived before the grace period"
    );
    assert!(
        family.log().contains("leader"),
        "the leader did not handle SIGTERM before SIGKILL"
    );
    // SIGKILL was sent before returning; the orphaned member is reaped by init.
    assert!(family.exited_within(Duration::from_secs(2)).await);
}

#[tokio::test]
async fn dropping_the_capture_terminates_the_group_in_the_background() {
    let (family, arguments) = Family::new("echo leader >> LOG", "");
    let grace = Duration::from_millis(400);
    let token = CancellationToken::new();
    {
        let operation = capture(&arguments, grace, &token);
        tokio::pin!(operation);
        tokio::select! {
            result = &mut operation => panic!("premature exit: {result:?}"),
            _ = family.started() => {}
        }
    }
    let dropped = Instant::now();
    assert!(family.exited_within(Duration::from_secs(5)).await);
    assert!(
        dropped.elapsed() >= grace / 2,
        "SIGKILL arrived without a grace period"
    );
    assert!(
        family.log().contains("leader"),
        "the leader did not handle SIGTERM before SIGKILL"
    );
}

#[tokio::test]
async fn output_failure_terminates_the_group() {
    let (family, mut arguments) = Family::new("echo leader >> LOG; exit 0", "exit 0");
    // The leader keeps printing; failing only once both processes have
    // installed their traps keeps SIGTERM out of the window between fork and
    // exec, where a shell can lose it.
    let script = arguments[1]
        .strip_suffix("while :; do sleep 1; done")
        .expect("the leader loop ends the script");
    arguments[1] = format!("{script}while :; do printf .; sleep 0.1; done");
    let result = process::capture_with(
        Path::new("/bin/sh"),
        &arguments,
        None,
        Duration::from_secs(30),
        &CancellationToken::new(),
        |_, _| {
            if family.member.exists() && family.leader.exists() {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed display").into())
            } else {
                Ok(())
            }
        },
    )
    .await;
    assert!(matches!(result, Err(Error::Io(_))));
    assert!(family.log().contains("leader"));
    assert!(family.exited_within(Duration::ZERO).await);
}

#[tokio::test]
async fn output_draining_is_bounded_after_the_leader_exits() {
    let directory = common::scratch();
    let member = directory.path().join("member");
    // The member inherits the output pipes and outlives the leader.
    let script = format!(
        "sh -c 'printf %s $$ > \"{member}\"; exec sleep 60' & printf done; exit 3",
        member = member.display()
    );
    let started = Instant::now();
    let output = capture(&["-c".into(), script], GRACE, &CancellationToken::new())
        .await
        .unwrap();
    let elapsed = started.elapsed();
    let id: libc::pid_t = std::fs::read_to_string(&member).unwrap().parse().unwrap();
    unsafe {
        libc::kill(id, libc::SIGKILL);
    }
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(output.stdout, b"done");
    assert!(
        elapsed >= Duration::from_secs(1) && elapsed < Duration::from_secs(10),
        "{elapsed:?}"
    );
}

#[tokio::test]
async fn a_member_flooding_output_cannot_hold_off_sigkill() {
    // The leader survives SIGTERM, and the member ignores it while writing
    // without pause.
    let (family, arguments) = Family::new("echo leader >> LOG", "");
    let arguments = vec![
        arguments[0].clone(),
        arguments[1].replace("while :; do sleep 1; done'", "yes & yes & yes & exec yes'"),
    ];
    let grace = Duration::from_millis(500);
    let token = CancellationToken::new();
    let operation = capture(&arguments, grace, &token);
    tokio::pin!(operation);
    tokio::select! {
        result = &mut operation => panic!("premature exit: {result:?}"),
        _ = family.started() => {}
    }
    let started = Instant::now();
    token.cancel();
    let result = tokio::time::timeout(Duration::from_secs(10), &mut operation)
        .await
        .expect("cancellation finished despite the flood");
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(started.elapsed() >= grace);
    assert!(family.exited_within(Duration::from_secs(2)).await);
}
