---
created_on: 2026-10-14 08:47
last_modified: 2026-10-14 16:43
status: current
---

# fetch-mix development

Read the parent instructions and applicable skills before editing. CLI work requires `cli-best-practices`; documentation requires `docs-writer`; usage skill edits require `skill-writer` and prompt construction requires `prompt-writer`.

## Development

- This is an independent Cargo package with sibling path dependencies `../deps`, `../cli-command-kit`, and `../cli-event-stream`.
- Build with `just build`; the development binary is `bin/fetch-mix`. Run with `just run <args>` or `just run-ai <args>`.
- Run `just check` for package-scoped formatting, strict Clippy, behavioral tests, and at least 90% line coverage. Coverage requires cargo-llvm-cov and llvm-tools.
- Develop runtime changes with failing behavioral tests, then temporarily disable the change to prove the regression fails, restore it, and rerun. Keep logs in `.tmp/`.
- Preserve the MIT license and source attribution. Keep Cargo.lock tracked. Do not publish or configure remotes without explicit authorization.
- Rust artifacts are unpublished. Development builds use the sibling libraries. A canonical Go repository or its releases does not establish the availability of Rust artifacts or the revised usage skill.

## Contracts

- Keep `skills/fetch-mix/SKILL.md`, its references, README, and live clap command tree synchronized whenever commands, aliases, arguments, flags, defaults, environments, output, errors, or side effects change. Run the skill drift test and skill-writer validator.
- Keep `skill` output exactly the compiled installation pointer in both modes; runtime must not read repository documentation.
- Use native genai structured output with separate system instructions and untrusted user data. Never expose credentials or use tracklist content as instructions.
- Print a returned error once in main; exit 130 on cancellation and 1 on other errors. Keep stdout exclusively NDJSON when it is the progress target.
- Preserve the native fetch-track 0.1.0 root workflow and NDJSON fields. Its result.path may contain `~/`; expand it before filesystem operations. Verify an actual nonempty regular artifact in the output directory before completing or resuming a track.
- Keep full track identity and order in resume validation; reject mismatched or corrupt manifests. Never overwrite another numbered artifact.
- Use `cli-event-stream::Reporter` directly for NDJSON transport, including readiness, duplicated descriptor ownership, and shared open-file flag restoration. Keep the mix event schema and timestamp enrichment in this package.
- Child process groups must be killed and leaders reaped on cancellation or output failure. Keep inherited-pipe draining bounded after leader exit.
- Drain queued native socket accepts before closing the progress listener. A fast child can connect/write/exit before the async accept task runs. Preserve the bounded 200 ms reader drain and regressions for both TCP and Unix sockets.
- Installer and updater tests use local HTTP archives and isolated HOME, XDG directories, and PATH. Do not install canonical Go archives as Rust binaries or perform live external writes.
- Explicit Rust release sources use all-or-none BASE_URL/OWNER/REPO environment variables. Parse channels with `cli_deps::ReleaseChannel` and update with `cli_deps::Updater`; retain typed current, installed, and unpublished outcomes. Dependency installation uses the native ReleaseClient archive integration.

## Native integration

`FETCH_MIX_NATIVE_TRACK=../fetch-track-cli/target/debug/fetch-track cargo test --test native_track -- --ignored` exercises the actual sibling Rust downloader and real ffmpeg/ffprobe on generated local audio. Build the sibling binary first and use an absolute executable path if running from another directory. The test isolates network/media fixtures and runtime directories; it does not verify external music providers or hosted AI services.
