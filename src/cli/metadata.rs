use super::*;
pub(super) fn ai(matches: &ArgMatches, mode: Mode, environment: &Environment) -> Result<()> {
    let ui = Ui::new(mode, std::io::stdout());
    let selected = matches
        .subcommand()
        .filter(|(name, _)| *name == "inspect")
        .and_then(|(_, args)| args.get_one::<String>("provider"));
    if matches.subcommand_name() == Some("inspect") && selected.is_none() {
        let mut command = command(mode);
        command
            .find_subcommand_mut("ai")
            .expect("AI group")
            .find_subcommand_mut("inspect")
            .expect("inspect")
            .print_help()?;
        println!();
        return Ok(());
    }
    let mut table = comfy_table::Table::new();
    table
        .set_header(["Provider", "Default model", "Environment", "Status"])
        .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
        .set_width(u16::try_from(cli_command_kit::terminal_width().max(80)).unwrap_or(u16::MAX));
    for state in llm::statuses(environment)
        .iter()
        .filter(|s| selected.is_none_or(|id| id == s.provider.id))
    {
        let provider = &state.provider;
        if selected.is_some() {
            ui.status("provider", provider.id)?;
            ui.status("name", provider.name)?;
            ui.status("default_model", &state.model)?;
            ui.status("env_var", provider.environment[0])?;
            ui.status("status", state.status)?;
        } else if mode == Mode::Human {
            table.add_row([
                provider.id,
                &state.model,
                provider.environment[0],
                state.status,
            ]);
        } else {
            ui.line(format!(
                "{}\t{}\t{}\t{}",
                provider.id, state.model, provider.environment[0], state.status
            ))?
        }
    }
    if selected.is_none() && mode == Mode::Human {
        ui.line(table.to_string())?
    }
    Ok(())
}
pub(super) async fn dependencies(
    matches: &ArgMatches,
    mode: Mode,
    environment: &Environment,
    token: &CancellationToken,
) -> Result<()> {
    let dependencies = Dependencies::new(environment)?;
    let ui = Ui::new(mode, std::io::stdout());
    let (action, args) = matches.subcommand().unwrap_or(("verify", matches));
    if action == "verify" {
        let reports = dependencies.manager.verify(token).await?;
        for report in &reports {
            ui.line(format!(
                "{}\t{}\t{}\t{}",
                report.name,
                report.detected_version,
                report.min_version,
                if report.satisfied { "OK" } else { "FAIL" }
            ))?
        }
        let missing = reports
            .iter()
            .filter(|r| !r.satisfied)
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(cli_deps::Error::Unsatisfied(missing.join(", ")).into());
        }
        return Ok(());
    }
    let names = args
        .get_many::<String>("dependency")
        .map(|values| values.cloned().collect::<Vec<_>>());
    let names = match names {
        Some(names) => names,
        None if action == "update" => NAMES.map(str::to_owned).to_vec(),
        None => dependencies
            .manager
            .verify(token)
            .await?
            .into_iter()
            .filter(|r| !r.satisfied)
            .map(|r| r.name)
            .collect(),
    };
    if names.is_empty() {
        ui.status("status", "satisfied")?
    }
    for name in names {
        dependencies
            .mutate(&name, action == "update", token)
            .await?;
        ui.status(
            if action == "update" {
                "updated"
            } else {
                "installed"
            },
            name,
        )?
    }
    Ok(())
}
