use crate::{
    CancellationToken, Error, Result,
    cache::Cache,
    dependencies::{Dependencies, NAMES, read_line},
    download::{self, DownloadOptions},
    llm::{self, Environment, Llm, ProviderConfig},
    model::SearchResult,
    progress::{Event, Reporter, Target},
    releases,
    ui::Ui,
    web::{self, Discovery, Endpoints},
    youtube,
};
use cli_command_kit::{
    HelpOptions, Mode,
    clap::{Arg, ArgAction, ArgMatches, Command, builder::PossibleValuesParser},
};
use std::{io::Write, path::PathBuf};
mod metadata;
mod pipeline;
use metadata::{ai, dependencies};
use pipeline::run_pipeline;
pub const SKILL_POINTER: &str = "Install the fetch-mix skill using the Skills CLI:\n\n  npx skills add alexgorbatchev/fetch-mix-cli --skill fetch-mix\n\nSkill:\n\n  https://github.com/alexgorbatchev/fetch-mix-cli/tree/main/skills/fetch-mix\n";
pub fn help_options() -> HelpOptions {
    let width = cli_command_kit::terminal_width();
    HelpOptions {
        skill: Some(SKILL_POINTER.into()),
        tree: cli_command_kit::TreeOptions {
            terminal_width: Some(if width == 0 { 80 } else { width }),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn flag(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .help(help)
        .action(ArgAction::SetTrue)
}
fn pipeline(command: Command) -> Command {
    command
        .arg(
            Arg::new("query")
                .value_name("query-or-url")
                .num_args(0..)
                .help("DJ set query, tracklist URL or YouTube video"),
        )
        .arg(
            Arg::new("out-dir")
                .long("out-dir")
                .short('o')
                .value_parser(cli_command_kit::clap::value_parser!(PathBuf))
                .help("Output directory; default is the current directory plus the mix title"),
        )
        .arg(flag("dry-run", "Preview tracks without downloading audio"))
        .arg(flag(
            "auto-install",
            "Install missing dependencies automatically",
        ))
        .arg(flag("no-cache", "Disable discovery and extraction caches"))
        .arg(flag("skip-verify", "Skip downloaded audio quality checks"))
        .arg(flag("skip-metadata", "Skip cover art and metadata tagging"))
        .arg(
            Arg::new("sources")
                .long("sources")
                .short('s')
                .default_value("youtube,soundcloud")
                .value_parser(validate_sources)
                .help("Comma-separated youtube and soundcloud search sources"),
        )
        .arg(
            Arg::new("llm-provider")
                .long("llm-provider")
                .short('p')
                .default_value("auto")
                .value_parser(PossibleValuesParser::new(
                    std::iter::once("auto")
                        .chain(llm::providers().iter().map(|p| p.id))
                        .collect::<Vec<_>>(),
                ))
                .help("AI provider or auto detection"),
        )
        .arg(
            Arg::new("llm-model")
                .long("llm-model")
                .short('m')
                .default_value("")
                .help("Override the provider's default model"),
        )
        .arg(
            Arg::new("progress-target")
                .long("progress-target")
                .value_parser(validate_target)
                .help("NDJSON target: stdout, stderr, unix://, tcp:// or fd://"),
        )
        .arg(
            Arg::new("progress-socket")
                .long("progress-socket")
                .value_parser(validate_target)
                .help("Alias target, used when --progress-target is absent"),
        )
}
fn validate_sources(value: &str) -> std::result::Result<String, String> {
    if value
        .split(',')
        .all(|s| matches!(s.trim(), "youtube" | "soundcloud"))
    {
        Ok(value.into())
    } else {
        Err("sources must contain youtube or soundcloud, separated by commas".into())
    }
}
fn validate_target(value: &str) -> std::result::Result<String, String> {
    Target::parse(value)
        .map(|_| value.into())
        .map_err(|e| e.to_string())
}
pub fn command(mode: Mode) -> Command {
    let ai = Command::new("ai")
        .visible_aliases(["providers", "models"])
        .about("Inspect AI providers")
        .subcommand(
            Command::new("list")
                .visible_alias("ls")
                .about("List supported AI providers"),
        )
        .subcommand(
            Command::new("inspect")
                .visible_aliases(["show", "get"])
                .about("Inspect a provider's configuration")
                .arg(Arg::new("provider").value_parser(llm::provider_id)),
        );
    let dependencies = Command::new("dependencies")
        .visible_alias("deps")
        .about("Manage external executables")
        .subcommand(
            Command::new("verify")
                .visible_aliases(["check", "status"])
                .about("Verify required executables and versions"),
        )
        .subcommand(
            Command::new("install")
                .visible_aliases(["add", "get"])
                .about("Install named or missing dependencies")
                .arg(Arg::new("dependency").num_args(0..).value_parser(NAMES)),
        )
        .subcommand(
            Command::new("update")
                .visible_alias("upgrade")
                .about("Update named or all dependencies")
                .arg(Arg::new("dependency").num_args(0..).value_parser(NAMES)),
        );
    let root = pipeline(
        Command::new("fetch-mix")
            .version(env!("CARGO_PKG_VERSION"))
            .about("Discover DJ set tracklists and download numbered playlists")
            .disable_version_flag(true),
    )
    .arg(
        Arg::new("version")
            .long("version")
            .action(ArgAction::SetTrue)
            .help("Print the raw version"),
    )
    .arg(flag("interactive", "Choose among matching DJ sets").short('i'))
    .arg(
        flag("verbose", "Show downloader diagnostics")
            .short('v')
            .global(true),
    )
    .subcommand(pipeline(
        Command::new("youtube")
            .visible_alias("yt")
            .about("Extract a YouTube comment tracklist"),
    ))
    .subcommand(ai)
    .subcommand(dependencies)
    .subcommand(
        Command::new("upgrade")
            .visible_aliases(["self-update", "update-self"])
            .about("Update from an explicitly configured Rust release source"),
    );
    cli_command_kit::setup(root, mode, &help_options()).expect("valid command tree")
}
pub async fn execute(
    matches: &ArgMatches,
    mode: Mode,
    environment: &Environment,
    token: &CancellationToken,
    endpoints: Endpoints,
) -> Result<()> {
    if matches.get_flag("version") {
        std::io::stdout().write_all(concat!(env!("CARGO_PKG_VERSION"), "\n").as_bytes())?;
        return Ok(());
    }
    if cli_command_kit::write_skill(matches, &help_options(), &mut std::io::stdout())? {
        return Ok(());
    }
    match matches.subcommand() {
        Some(("ai", args)) => ai(args, mode, environment),
        Some(("dependencies", args)) => dependencies(args, mode, environment, token).await,
        Some(("upgrade", _)) => {
            let updater = cli_deps::Updater {
                channel: releases::from_environment("FETCH_MIX", environment)?,
            };
            let result = updater
                .update_running(env!("CARGO_PKG_VERSION"), token)
                .await?;
            let (status, version) = match result {
                cli_deps::Update::Current(version) => ("current", version),
                cli_deps::Update::Installed(version) => ("upgraded", version),
            };
            let ui = Ui::new(mode, std::io::stdout());
            ui.status("status", status)?;
            ui.status("version", version)
        }
        Some(("youtube", args)) => {
            run_pipeline(args, mode, environment, true, token, endpoints).await
        }
        _ => run_pipeline(matches, mode, environment, false, token, endpoints).await,
    }
}
