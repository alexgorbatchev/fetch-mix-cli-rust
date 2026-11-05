use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
// Integration tests compile this shared fixture independently; download tests
// use the executable fixture without constructing an HTTP server.
#[allow(dead_code)]
pub struct Http {
    pub url: String,
    stop: Arc<AtomicBool>,
    task: Option<std::thread::JoinHandle<()>>,
}
#[allow(dead_code)]
impl Http {
    pub fn new(handler: impl Fn(tiny_http::Request) + Send + 'static) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let task = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                if let Some(request) = server
                    .recv_timeout(std::time::Duration::from_millis(20))
                    .unwrap()
                {
                    handler(request)
                }
            }
        });
        Self {
            url,
            stop,
            task: Some(task),
        }
    }
}
impl Drop for Http {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.task.take().unwrap().join().unwrap();
    }
}
#[allow(dead_code)]
pub fn json(request: tiny_http::Request, value: serde_json::Value) {
    request
        .respond(
            tiny_http::Response::from_string(value.to_string()).with_header(
                tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
            ),
        )
        .unwrap();
}
/// Test-owned scratch inside the runner's TMPDIR or the project's `.tmp`.
#[allow(dead_code)]
pub fn scratch() -> tempfile::TempDir {
    let root = std::env::var_os("TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp"));
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}
#[allow(dead_code)]
pub fn track_fixture(directory: &std::path::Path) -> std::path::PathBuf {
    let file = directory.join(format!("track-fixture{}", std::env::consts::EXE_SUFFIX));
    if !file.exists() {
        let status = std::process::Command::new("rustc")
            .arg("--edition=2024")
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/track.rs"))
            .arg("-o")
            .arg(&file)
            .status()
            .unwrap();
        assert!(status.success());
    }
    file
}
#[allow(dead_code)]
pub fn archive(binary: &str, bytes: &[u8]) -> Vec<u8> {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut archive = tar::Builder::new(&mut gzip);
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append_data(&mut header, binary, bytes).unwrap();
        archive.finish().unwrap();
    }
    gzip.finish().unwrap()
}
