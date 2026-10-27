use crate::{CancellationToken, Error, Result};
pub use cli_event_stream::{Reporter, Target};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::AsyncRead,
    net::TcpListener,
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_util::codec::{FramedRead, LinesCodec};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Event {
    #[serde(default)]
    pub timestamp: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub phase: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}
impl Event {
    pub fn stamped(mut self) -> Self {
        if self.timestamp.is_empty() {
            self.timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        }
        self
    }
    pub fn new(kind: &str, phase: &str) -> Self {
        Self {
            kind: kind.into(),
            phase: phase.into(),
            ..Self::default()
        }
    }
}

enum Listener {
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),
}
impl Listener {
    async fn accept(&self) -> std::io::Result<Box<dyn AsyncRead + Unpin + Send>> {
        match self {
            Self::Tcp(l) => Ok(Box::new(l.accept().await?.0)),
            #[cfg(unix)]
            Self::Unix(l) => Ok(Box::new(l.accept().await?.0)),
        }
    }
    fn drain_queued(
        self,
        mut receive: impl FnMut(Box<dyn AsyncRead + Unpin + Send>),
    ) -> std::io::Result<()> {
        // The child may connect, send, and exit before Tokio polls accept.
        // Consume the kernel backlog directly before closing the listener;
        // polling an async accept once can miss unobserved reactor readiness.
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        match self {
            Self::Tcp(listener) => {
                let listener = listener.into_std()?;
                while std::time::Instant::now() < deadline {
                    match listener.accept() {
                        Ok((peer, _)) => {
                            peer.set_nonblocking(true)?;
                            receive(Box::new(tokio::net::TcpStream::from_std(peer)?));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => return Err(error),
                    }
                }
            }
            #[cfg(unix)]
            Self::Unix(listener) => {
                let listener = listener.into_std()?;
                while std::time::Instant::now() < deadline {
                    match listener.accept() {
                        Ok((peer, _)) => {
                            peer.set_nonblocking(true)?;
                            receive(Box::new(tokio::net::UnixStream::from_std(peer)?));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        Ok(())
    }
}
pub struct Server {
    pub target: String,
    socket: Option<PathBuf>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
    receiver: mpsc::Receiver<Result<Event>>,
}
impl Server {
    pub async fn start(
        directory: &Path,
        force_tcp: bool,
        parent: &CancellationToken,
    ) -> Result<Self> {
        let mut socket = None;
        let mut unix = None;
        #[cfg(unix)]
        if !force_tcp && std::fs::create_dir_all(directory).is_ok() {
            let path = directory.join(format!(
                "fm_{}_{:x}.sock",
                std::process::id(),
                fastrand::u64(..)
            ));
            if let Ok(listener) = tokio::net::UnixListener::bind(&path) {
                unix = Some(Listener::Unix(listener));
                socket = Some(path);
            }
        }
        #[cfg(not(unix))]
        let _ = (directory, force_tcp);
        let (listener, target) = if let Some(listener) = unix {
            (
                listener,
                format!(
                    "unix://{}",
                    socket.as_ref().expect("bound socket").display()
                ),
            )
        } else {
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let target = format!("tcp://{}", listener.local_addr()?);
            (Listener::Tcp(listener), target)
        };
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let parent = parent.clone();
        let (sender, receiver) = mpsc::channel(128);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! { biased;
                    _=parent.cancelled()=>break,
                    _=task_stop.cancelled()=>break,
                    accepted=listener.accept()=>match accepted {Ok(peer)=>{let sender=sender.clone();let token=parent.clone();connections.spawn(async move { read_peer(peer,sender,token).await; });},Err(_)=>break},
                    Some(_)=connections.join_next()=>{},
                }
            }
            if !parent.is_cancelled() {
                let drained = listener.drain_queued(|peer| {
                    let sender = sender.clone();
                    let token = parent.clone();
                    connections.spawn(async move {
                        read_peer(peer, sender, token).await;
                    });
                });
                if let Err(error) = drained {
                    let _ = sender.send(Err(error.into())).await;
                }
            }
            drop(sender);
            // A finished child closes its socket. Bound drain time also handles inherited or malicious open peers.
            let drain = async { while connections.join_next().await.is_some() {} };
            tokio::select! {_=parent.cancelled()=>{},_=tokio::time::timeout(Duration::from_millis(200),drain)=>{}}
        });
        Ok(Self {
            target,
            socket,
            stop,
            task: Some(task),
            receiver,
        })
    }
    pub async fn next(&mut self) -> Option<Result<Event>> {
        self.receiver.recv().await
    }
    pub async fn finish(mut self) -> Result<Vec<Event>> {
        self.stop.cancel();
        let mut events = Vec::new();
        while let Some(event) = self.receiver.recv().await {
            events.push(event?)
        }
        if let Some(task) = self.task.take() {
            task.await
                .map_err(|e| Error::Invalid(format!("progress server: {e}")))?
        }
        Ok(events)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort()
        }
        if let Some(path) = &self.socket {
            let _ = std::fs::remove_file(path);
        }
    }
}
async fn read_peer(
    peer: Box<dyn AsyncRead + Send + Unpin>,
    sender: mpsc::Sender<Result<Event>>,
    token: CancellationToken,
) {
    let mut reader = FramedRead::new(peer, LinesCodec::new_with_max_length(1024 * 1024));
    loop {
        let line = tokio::select! {_=token.cancelled()=>return,result=reader.next()=>result};
        let line = match line {
            None => return,
            Some(Ok(line)) => line,
            Some(Err(error)) => {
                let _ = sender
                    .send(Err(Error::Invalid(format!(
                        "invalid progress frame: {error}"
                    ))))
                    .await;
                return;
            }
        };
        if let Ok(event) = serde_json::from_str(&line) {
            tokio::select! {_=token.cancelled()=>return,_=sender.send(Ok(event))=>{}}
        }
    }
}
