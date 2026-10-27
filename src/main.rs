use fetch_mix_cli::{CancellationToken, Error, cli, llm::Environment};
#[tokio::main]
async fn main() {
    let mode = cli_command_kit::Mode::from_env();
    let matches = match cli::command(mode).try_get_matches() {
        Ok(matches) => matches,
        Err(error) => {
            let code = if error.use_stderr() { 1 } else { 0 };
            let _ = error.print();
            std::process::exit(code)
        }
    };
    let token = CancellationToken::new();
    let signals = token.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut interrupt =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .expect("interrupt handler");
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("terminate handler");
            tokio::select! {_=interrupt.recv()=>{},_=terminate.recv()=>{}}
            signals.cancel();
            tokio::select! {_=interrupt.recv()=>{},_=terminate.recv()=>{}}
            std::process::exit(130);
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            signals.cancel();
            let _ = tokio::signal::ctrl_c().await;
            std::process::exit(130);
        }
    });
    if let Err(error) = cli::execute(
        &matches,
        mode,
        &std::env::vars().collect::<Environment>(),
        &token,
        fetch_mix_cli::web::Endpoints::default(),
    )
    .await
    {
        eprintln!(
            "{}: {}",
            if mode == cli_command_kit::Mode::Agent {
                "ERR"
            } else {
                "[ERROR]"
            },
            fetch_mix_cli::ui::Ui::fold_text(&error.to_string())
        );
        let code = if token.is_cancelled()
            || matches!(
                error,
                Error::Cancelled | Error::Dependencies(cli_deps::Error::Cancelled)
            ) {
            130
        } else {
            1
        };
        std::process::exit(code)
    }
}
