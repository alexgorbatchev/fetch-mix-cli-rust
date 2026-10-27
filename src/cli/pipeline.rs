use super::*;
async fn report(
    reporter: Option<&Reporter>,
    kind: &str,
    phase: &str,
    message: impl Into<String>,
    token: &CancellationToken,
) -> Result<()> {
    if let Some(reporter) = reporter {
        let mut event = Event::new(kind, phase);
        event.message = message.into();
        reporter.emit(event.stamped(), token).await?
    }
    Ok(())
}
pub(super) async fn run_pipeline(
    matches: &ArgMatches,
    mode: Mode,
    environment: &Environment,
    force_youtube: bool,
    token: &CancellationToken,
    endpoints: Endpoints,
) -> Result<()> {
    let query = matches
        .get_many::<String>("query")
        .map(|s| s.map(String::as_str).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if query.trim().is_empty() {
        let mut command = command(mode);
        if force_youtube {
            command
                .find_subcommand_mut("youtube")
                .expect("youtube")
                .print_help()?
        } else {
            command.print_help()?
        }
        println!();
        return Ok(());
    }
    let target = matches
        .get_one::<String>("progress-target")
        .or_else(|| matches.get_one::<String>("progress-socket"))
        .or_else(|| environment.get("FETCH_MIX_PROGRESS_TARGET"))
        .filter(|s| !s.trim().is_empty());
    let reporter = if let Some(target) = target {
        Some(Reporter::connect(&Target::parse(target)?, token).await?)
    } else {
        None
    };
    let ui = if target.is_some_and(|t| matches!(Target::parse(t), Ok(Target::Stdout))) {
        Ui::new(mode, Box::new(std::io::stderr()) as Box<dyn Write + Send>)
    } else {
        Ui::new(mode, Box::new(std::io::stdout()) as Box<dyn Write + Send>)
    };
    let dry_run = matches.get_flag("dry-run");
    let dependencies = if dry_run {
        None
    } else {
        let deps = Dependencies::new(environment)?;
        deps.ensure(matches.get_flag("auto-install"), &ui, token)
            .await?;
        Some(deps)
    };
    let cache = (!matches.get_flag("no-cache")).then(Cache::resolve);
    let provider = matches
        .get_one::<String>("llm-provider")
        .expect("provider default");
    let model = matches
        .get_one::<String>("llm-model")
        .expect("model default");
    let config = if provider == "auto"
        && !llm::providers()
            .iter()
            .any(|p| llm::configured(p, environment))
    {
        None
    } else {
        Some(ProviderConfig::resolve(provider, model, environment)?)
    };
    let llm = if let Some(config) = config {
        Some(Llm::new(config).await?)
    } else {
        None
    };
    let youtube = force_youtube || youtube::video_id(&query).is_some();
    let (title, tracks, skipped) = if youtube {
        report(
            reporter.as_ref(),
            "phase_start",
            "youtube_comments",
            format!("Processing {query}"),
            token,
        )
        .await?;
        let executable = dependencies
            .as_ref()
            .map(|d| d.executable("yt-dlp"))
            .transpose()?
            .unwrap_or_else(|| PathBuf::from("yt-dlp"));
        let result = youtube::process(
            &query,
            &executable,
            dependencies.as_ref().map(|d| d.path.as_os_str()),
            cache.as_ref(),
            llm.as_ref(),
            token,
        )
        .await?;
        (result.video_title, result.tracks, result.skipped_items)
    } else {
        let discovery = Discovery::new(endpoints)?;
        let direct = reqwest::Url::parse(&query).ok().is_some_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && !u
                    .host_str()
                    .is_some_and(|h| matches!(h, "1001tracklists.com" | "www.1001tracklists.com"))
        });
        let selected = if direct {
            SearchResult {
                title: web::title_from_url(&query),
                url: query.clone(),
            }
        } else {
            report(
                reporter.as_ref(),
                "phase_start",
                "search",
                format!("Searching {query}"),
                token,
            )
            .await?;
            let results = discovery.search(&query, token).await?;
            if results.is_empty() {
                return Err(Error::Invalid(format!(
                    "no matching DJ sets found for {query:?}"
                )));
            }
            for (index, result) in results.iter().enumerate() {
                ui.line(format!("[{}] {}", index + 1, result.title))?
            }
            let index =
                if results.len() > 1 && mode == Mode::Human && matches.get_flag("interactive") {
                    ui.line(format!("Choose option (1-{}) [1]:", results.len()))?;
                    read_line(token)
                        .await?
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|i| *i > 0 && *i <= results.len())
                        .unwrap_or(1)
                        - 1
                } else {
                    0
                };
            results[index].clone()
        };
        ui.status("selected", &selected.title)?;
        report(
            reporter.as_ref(),
            "phase_start",
            "scrape",
            format!("Scraping {}", selected.url),
            token,
        )
        .await?;
        let markdown = discovery
            .scrape(&selected.url, cache.as_ref(), token)
            .await?;
        let extraction = if let Some(llm) = &llm {
            match llm.extract_content(&markdown, &selected.title, token).await {
                Ok(value) if !value.tracks.is_empty() => Some(value),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                _ => None,
            }
        } else {
            None
        };
        let (tracks, skipped) = if let Some(value) = extraction {
            (value.tracks, value.skipped_items)
        } else {
            crate::parser::parse(&markdown)
        };
        if tracks.is_empty() {
            return Err(Error::Invalid(
                "could not extract any tracks from the tracklist page".into(),
            ));
        }
        (selected.title, tracks, skipped)
    };
    report(
        reporter.as_ref(),
        "progress",
        "parse",
        format!("Extracted {} tracks", tracks.len()),
        token,
    )
    .await?;
    let options = DownloadOptions {
        title,
        tracks,
        skipped,
        output: matches.get_one::<PathBuf>("out-dir").cloned(),
        dry_run,
        sources: matches
            .get_one::<String>("sources")
            .expect("sources default")
            .clone(),
        skip_verify: matches.get_flag("skip-verify"),
        skip_metadata: matches.get_flag("skip-metadata"),
        verbose: matches.get_flag("verbose"),
        reporter,
        executable: dependencies
            .as_ref()
            .map(|d| d.executable("fetch-track"))
            .transpose()?
            .unwrap_or_else(|| "fetch-track".into()),
        path: dependencies.map(|d| d.path),
        ..Default::default()
    };
    download::download(&options, &ui, token).await
}
