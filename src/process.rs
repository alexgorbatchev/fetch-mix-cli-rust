use crate::{CancellationToken, Error, Result};
use std::{
    ffi::OsStr,
    path::Path,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::io::AsyncReadExt;
pub async fn capture(
    program: &Path,
    args: &[String],
    path: Option<&OsStr>,
    token: &CancellationToken,
) -> Result<Output> {
    capture_with(program, args, path, token, |_, _| Ok(())).await
}
/// Reads both child pipes while waiting, reporting bytes as soon as they arrive.
pub async fn capture_with(
    program: &Path,
    args: &[String],
    path: Option<&OsStr>,
    token: &CancellationToken,
    mut output: impl FnMut(&[u8], bool) -> Result<()>,
) -> Result<Output> {
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    let mut group = Group {
        pid: child.id(),
        armed: true,
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut captured_out = Vec::new();
    let mut captured_err = Vec::new();
    let mut status = None;
    let mut out_closed = false;
    let mut err_closed = false;
    let mut out_buffer = [0; 8192];
    let mut err_buffer = [0; 8192];
    let mut deadline = None;
    loop {
        if out_closed && err_closed && status.is_some() {
            break;
        }
        tokio::select! {biased;
         _=token.cancelled()=>{group.kill();let _=tokio::time::timeout(Duration::from_secs(2),child.kill()).await;return Err(Error::Cancelled)},
         value=stdout.read(&mut out_buffer),if !out_closed=>{let count=value?;out_closed=count==0;if count>0{captured_out.extend_from_slice(&out_buffer[..count]);output(&out_buffer[..count],false)?}},
         value=stderr.read(&mut err_buffer),if !err_closed=>{let count=value?;err_closed=count==0;if count>0{captured_err.extend_from_slice(&err_buffer[..count]);output(&err_buffer[..count],true)?}},
         value=child.wait(),if status.is_none()=>{status=Some(value?);deadline=Some(tokio::time::Instant::now()+Duration::from_secs(2));},
         _=async{if let Some(deadline)=deadline{tokio::time::sleep_until(deadline).await}else{std::future::pending::<()>().await}},if status.is_some()=>break,
        }
    }
    group.armed = false;
    Ok(Output {
        status: status.expect("child exited before drain"),
        stdout: captured_out,
        stderr: captured_err,
    })
}
struct Group {
    pid: Option<u32>,
    armed: bool,
}
impl Group {
    fn kill(&mut self) {
        if self.armed {
            #[cfg(unix)]
            if let Some(pid) = self.pid {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            self.armed = false;
        }
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        self.kill()
    }
}
