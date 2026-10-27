pub mod cache;
pub mod cli;
pub mod dependencies;
pub mod download;
pub mod llm;
pub mod manifest;
pub mod model;
pub mod parser;
pub mod process;
pub mod progress;
pub mod releases;
pub mod ui;
pub mod web;
pub mod youtube;
pub use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("Rust releases are unpublished; configure an explicit Rust release source for {0}")]
    Unpublished(String),
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Dependencies(#[from] cli_deps::Error),
    #[error("AI extraction failed: {0}")]
    Llm(#[from] genai::Error),
    #[error("{failed} of {total} tracks failed; inspect mix_manifest.json before retrying")]
    Partial { failed: usize, total: usize },
}
pub type Result<T> = std::result::Result<T, Error>;

pub async fn cancellable<T>(
    token: &CancellationToken,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! { biased; _ = token.cancelled() => Err(Error::Cancelled), value = future => value }
}

impl From<cli_event_stream::Error> for Error {
    fn from(error: cli_event_stream::Error) -> Self {
        match error {
            cli_event_stream::Error::Invalid(message) => Self::Invalid(message),
            cli_event_stream::Error::Cancelled => Self::Cancelled,
            cli_event_stream::Error::Io(error) => Self::Io(error),
            cli_event_stream::Error::Json(error) => Self::Json(error),
        }
    }
}
