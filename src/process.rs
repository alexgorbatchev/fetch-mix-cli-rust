use crate::{CancellationToken, Error, Result};
use std::{
    ffi::OsStr,
    path::Path,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::io::AsyncReadExt;

/// Time a cancelled or abandoned process group gets to exit after SIGTERM
/// before it receives SIGKILL.
pub const TERMINATION_GRACE: Duration = cli_deps::DEFAULT_TERMINATION_GRACE;

/// How long output pipes inherited by surviving group members are drained after
/// the leader exits.
const DRAIN: Duration = Duration::from_secs(2);

/// Runs `program` and reads both pipes while waiting, reporting bytes as soon
/// as they arrive.
///
/// On Unix the child leads its own process group. Cancellation, a failed read,
/// or an error from `output` ends the group: every member receives SIGTERM, and
/// the group receives SIGKILL only when a member is still alive after `grace`.
/// The call returns once the group is empty, or has been sent SIGKILL and its
/// leader reaped. Dropping the future starts the same sequence on a background
/// thread. On other platforms the child is killed.
///
/// The group does not receive the terminal's Ctrl-C, so callers translate
/// SIGINT and SIGTERM into cancelling `token`.
pub async fn capture_with(
    program: &Path,
    args: &[String],
    path: Option<&OsStr>,
    grace: Duration,
    token: &CancellationToken,
    mut output: impl FnMut(&[u8], bool) -> Result<()>,
) -> Result<Output> {
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(not(unix))]
    command.kill_on_drop(true);
    let mut group = Group::spawn(command, grace)?;
    match group.capture(token, &mut output).await {
        Ok(result) => {
            group.release();
            Ok(result)
        }
        Err(error) => {
            group.terminate().await;
            Err(error)
        }
    }
}

/// A spawned child whose process group is terminated unless it is released.
struct Group {
    child: Option<tokio::process::Child>,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    /// The leader's ID, which is also the group ID. Recorded at spawn because
    /// Tokio forgets it once the leader is reaped while other members may
    /// still be running.
    #[cfg(unix)]
    id: Option<libc::pid_t>,
    grace: Duration,
}

impl Group {
    fn spawn(mut command: tokio::process::Command, grace: Duration) -> std::io::Result<Self> {
        let mut child = command.spawn()?;
        Ok(Self {
            #[cfg(unix)]
            id: child.id().and_then(|id| libc::pid_t::try_from(id).ok()),
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            child: Some(child),
            grace,
        })
    }

    async fn capture(
        &mut self,
        token: &CancellationToken,
        output: &mut impl FnMut(&[u8], bool) -> Result<()>,
    ) -> Result<Output> {
        let Self {
            child,
            stdout,
            stderr,
            ..
        } = self;
        let child = child
            .as_mut()
            .expect("the child is present until released or terminated");
        let stdout = stdout.as_mut().expect("piped stdout");
        let stderr = stderr.as_mut().expect("piped stderr");
        let mut captured_out = Vec::new();
        let mut captured_err = Vec::new();
        let mut status = None;
        let mut out_closed = false;
        let mut err_closed = false;
        let mut out_buffer = [0; 8192];
        let mut err_buffer = [0; 8192];
        let mut deadline = None;
        while !(out_closed && err_closed && status.is_some()) {
            tokio::select! {
                biased;
                _ = token.cancelled() => return Err(Error::Cancelled),
                value = stdout.read(&mut out_buffer), if !out_closed => {
                    let count = value?;
                    out_closed = count == 0;
                    if count > 0 {
                        captured_out.extend_from_slice(&out_buffer[..count]);
                        output(&out_buffer[..count], false)?
                    }
                }
                value = stderr.read(&mut err_buffer), if !err_closed => {
                    let count = value?;
                    err_closed = count == 0;
                    if count > 0 {
                        captured_err.extend_from_slice(&err_buffer[..count]);
                        output(&err_buffer[..count], true)?
                    }
                }
                value = child.wait(), if status.is_none() => {
                    status = Some(value?);
                    deadline = Some(tokio::time::Instant::now() + DRAIN);
                }
                _ = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                }, if status.is_some() => break,
            }
        }
        Ok(Output {
            status: status.expect("the leader exited before draining ended"),
            stdout: captured_out,
            stderr: captured_err,
        })
    }

    /// Leaves members that outlived a finished leader running.
    fn release(&mut self) {
        self.child = None;
    }

    async fn terminate(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        #[cfg(unix)]
        {
            let (id, grace) = (self.id, self.grace);
            let pipes = Pipes::new(self.stdout.take(), self.stderr.take());
            signal_group(id, libc::SIGTERM);
            let _ = tokio::task::spawn_blocking(move || reap_group(child, id, grace, pipes)).await;
        }
        #[cfg(not(unix))]
        {
            let mut child = child;
            let _ = child.kill().await;
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        // On other platforms `kill_on_drop` kills the child as it drops.
        #[cfg(unix)]
        if let Some(child) = self.child.take() {
            let (id, grace) = (self.id, self.grace);
            let pipes = Pipes::new(self.stdout.take(), self.stderr.take());
            signal_group(id, libc::SIGTERM);
            let reaper = std::thread::Builder::new()
                .name("fetch-mix-reaper".into())
                .spawn(move || reap_group(child, id, grace, pipes));
            if reaper.is_err() {
                signal_group(id, libc::SIGKILL);
            }
        }
    }
}

/// The read ends of the child's output pipes, kept open and emptied while the
/// group exits so that members writing on their way out neither block nor
/// receive SIGPIPE.
#[cfg(unix)]
struct Pipes(Vec<std::fs::File>);

#[cfg(unix)]
impl Pipes {
    fn new(
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
    ) -> Self {
        use std::os::fd::AsRawFd;
        let descriptors = [
            stdout.map(tokio::process::ChildStdout::into_owned_fd),
            stderr.map(tokio::process::ChildStderr::into_owned_fd),
        ];
        Self(
            descriptors
                .into_iter()
                .flatten()
                .filter_map(std::io::Result::ok)
                .filter(|fd| {
                    // SAFETY: fcntl(2) on a descriptor this function owns.
                    unsafe {
                        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
                        flags >= 0
                            && libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                                >= 0
                    }
                })
                .map(std::fs::File::from)
                .collect(),
        )
    }

    /// Discards buffered output, closing pipes that reached end of file.
    fn drain(&mut self) {
        use std::io::Read;
        let mut buffer = [0; 8192];
        self.0.retain_mut(|pipe| {
            loop {
                match pipe.read(&mut buffer) {
                    Ok(0) => break false,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => break error.kind() == std::io::ErrorKind::WouldBlock,
                }
            }
        });
    }
}

#[cfg(unix)]
fn signal_group(id: Option<libc::pid_t>, signal: libc::c_int) {
    if let Some(id) = id {
        // SAFETY: kill(2) takes no pointers. A negative ID addresses the group,
        // and ESRCH means every member already exited.
        unsafe {
            libc::kill(-id, signal);
        }
    }
}

#[cfg(unix)]
fn group_alive(id: Option<libc::pid_t>) -> bool {
    // SAFETY: signal 0 only checks that the group still has a member.
    id.is_some_and(|id| unsafe { libc::kill(-id, 0) } == 0)
}

/// Waits up to `grace` for the leader and the rest of its group to exit, then
/// sends SIGKILL to the group and reaps the leader.
///
/// A group ID is not reused while the group has a member, and polling stops as
/// soon as the group is empty, so the signals reach only this group.
#[cfg(unix)]
fn reap_group(
    mut child: tokio::process::Child,
    id: Option<libc::pid_t>,
    grace: Duration,
    mut pipes: Pipes,
) {
    const POLL: Duration = Duration::from_millis(20);
    const KILL_WAIT: Duration = Duration::from_secs(1);
    let deadline = std::time::Instant::now() + grace;
    loop {
        pipes.drain();
        let leader_exited = !matches!(child.try_wait(), Ok(None));
        if leader_exited && !group_alive(id) {
            return;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(POLL);
    }
    signal_group(id, libc::SIGKILL);
    // Kill the leader directly too in case it left its group, then reap it.
    let _ = child.start_kill();
    let deadline = std::time::Instant::now() + KILL_WAIT;
    while matches!(child.try_wait(), Ok(None)) && std::time::Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
}
