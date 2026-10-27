use cli_command_kit::Mode;
use fetch_mix_cli::cli;

#[test]
fn usage_guide_tracks_live_commands_and_options() {
    let guide = std::fs::read_to_string("skills/fetch-mix/SKILL.md").unwrap();
    let setup = std::fs::read_to_string("skills/fetch-mix/references/setup.md").unwrap();
    let combined = format!("{guide}\n{setup}");
    let omissions = cli_command_kit::skill_omissions(
        &cli::command(Mode::Agent),
        &cli::help_options(),
        &combined,
    );
    assert!(omissions.is_empty(), "{omissions:#?}");
    for mode in [Mode::Human, Mode::Agent] {
        let matches = cli::command(mode)
            .try_get_matches_from(["fetch-mix", "skill"])
            .unwrap();
        let mut output = Vec::new();
        assert!(cli_command_kit::write_skill(&matches, &cli::help_options(), &mut output).unwrap());
        assert_eq!(String::from_utf8(output).unwrap(), cli::SKILL_POINTER);
        assert!(
            cli::command(mode)
                .try_get_matches_from(["fetch-mix", "skill", "extra"])
                .is_err()
        );
    }
}
