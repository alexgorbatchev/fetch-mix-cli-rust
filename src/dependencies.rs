use crate::{CancellationToken, Error, Result, llm::Environment, releases, ui::Ui};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};
pub const NAMES: [&str; 3] = ["fetch-track", "yt-dlp", "ffmpeg"];
pub struct Dependencies {
    pub manager: cli_deps::Manager,
    pub path: OsString,
    pub track_release: Option<cli_deps::ReleaseChannel>,
}
impl Dependencies {
    pub fn new(environment: &Environment) -> Result<Self> {
        let directory = cli_deps::ensure_managed_bin_dir("fetch-mix")?;
        let path = cli_deps::managed_path(&directory, std::env::var_os("PATH").as_deref())?;
        let runner = Arc::new(cli_deps::ProcessRunner {
            path: Some(path.clone()),
        });
        let release = releases::from_environment("FETCH_TRACK", environment)?;
        let mut track = cli_deps::Dependency::new("fetch-track", "0.1.0");
        track.install_url = "an explicitly configured Rust release source".into();
        if let Some(source) = &release {
            track.installer = Some(Arc::new(cli_deps::ReleaseInstaller {
                client: source.client.clone(),
                owner: source.owner.clone(),
                repo: source.repository.clone(),
                binary: "fetch-track".into(),
            }));
        }
        let mut youtube = cli_deps::Dependency::new("yt-dlp", "2024.08.01");
        youtube.install_url = "https://github.com/yt-dlp/yt-dlp#installation".into();
        youtube.installer = Some(Arc::new(cli_deps::YtDlpInstaller {
            client: cli_deps::ReleaseClient::github()?,
            runner: runner.clone(),
        }));
        let mut ffmpeg = cli_deps::Dependency::new("ffmpeg", "4.4");
        ffmpeg.version_args = vec!["-version".into()];
        ffmpeg.install_url = "https://ffmpeg.org/download.html".into();
        ffmpeg.installer = Some(Arc::new(cli_deps::PackageInstaller {
            package: "ffmpeg".into(),
            runner: runner.clone(),
        }));
        let manager = cli_deps::Manager::new(cli_deps::Config {
            app_name: "fetch-mix".into(),
            dependencies: vec![track, youtube, ffmpeg],
            runner: Some(runner),
            bin_dir: Some(directory),
            cache: None,
        })?;
        Ok(Self {
            manager,
            path,
            track_release: release,
        })
    }
    pub fn executable(&self, name: &str) -> Result<PathBuf> {
        which::which_in(name, Some(&self.path), std::env::current_dir()?)
            .map_err(|e| Error::Invalid(format!("{name} unavailable: {e}")))
    }
    pub async fn mutate(&self, name: &str, update: bool, token: &CancellationToken) -> Result<()> {
        if name == "fetch-track" && self.track_release.is_none() {
            return Err(Error::Unpublished("fetch-track".into()));
        }
        if update {
            self.manager.update(name, token).await?
        } else {
            self.manager.install(name, token).await?
        }
        Ok(())
    }
    pub async fn ensure(
        &self,
        auto_install: bool,
        ui: &Ui,
        token: &CancellationToken,
    ) -> Result<()> {
        let missing = self
            .manager
            .verify(token)
            .await?
            .into_iter()
            .filter(|r| !r.satisfied)
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        for report in &missing {
            ui.status("dependency", report.summary())?
        }
        let install = if auto_install {
            true
        } else if ui.mode == cli_command_kit::Mode::Agent {
            false
        } else {
            ui.line("Install missing dependencies into the managed directory? [y/N]")?;
            matches!(
                read_line(token).await?.trim().to_ascii_lowercase().as_str(),
                "y" | "yes"
            )
        };
        if !install {
            return Err(cli_deps::Error::Unsatisfied(
                missing
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .into());
        }
        for report in missing {
            self.mutate(&report.name, false, token).await?;
            ui.status("installed", report.name)?
        }
        let missing = self
            .manager
            .verify(token)
            .await?
            .into_iter()
            .filter(|r| !r.satisfied)
            .map(|r| r.summary())
            .collect::<Vec<_>>();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(cli_deps::Error::Unsatisfied(missing.join(", ")).into())
        }
    }
    pub fn bin_dir(&self) -> &Path {
        self.manager.bin_dir()
    }
}
pub async fn read_line(token: &CancellationToken) -> Result<String> {
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }
    // A detached native input thread avoids Tokio's uninterruptible stdin task
    // delaying runtime shutdown after cancellation. Only one prompt is active.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::stdin().read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    crate::cancellable(token, async {
        Ok(receiver
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))??)
    })
    .await
}
