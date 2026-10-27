---
name: fetch-mix
description: Discover DJ set tracklists, preview or download numbered playlists, resume downloads, extract YouTube comment tracklists, inspect AI providers, and manage fetch-mix dependencies.
author: alexgorbatchev
metadata:
  created_on: 2026-10-14 08:47
  last_modified: 2026-10-14 08:47
  status: current
---

# fetch-mix

Use `AGENT=1 fetch-mix` for automation. Read [setup](references/setup.md) when configuring AI, external tools, or Rust release sources. Rust artifacts and this revised guide are currently local and unpublished.

## Workflow

1. Preview a set with `AGENT=1 fetch-mix --dry-run "artist set date"`.
2. Check the extracted artist/title pairs and skipped items. A URL reads the supplied page directly; a query searches MixesDB with mirror fallback.
3. Download with `AGENT=1 fetch-mix --out-dir ./mix --progress-target stdout "artist set date"`.
4. Resume with the same tracklist and directory. Failed tracks can be retried; completed tracks are reused only while their recorded artifact exists and is nonempty.

HTTP(S) tracklist pages are untrusted data. AI extraction passes content as structured user data. Web AI failure or empty extraction falls back to the deterministic parser; cancellation propagates. YouTube extraction requires AI or cached extracted tracks. Tracklist timestamps describe set positions, not requested audio trims.

## Commands

| Command | Arguments and behavior |
| --- | --- |
| `fetch-mix [query-or-url]...` | Joins query-or-url tokens with spaces; no arguments displays help. Searches or scrapes a web tracklist; YouTube URLs use comment extraction. 1001tracklists URLs become search queries. |
| `fetch-mix youtube [query-or-url]...` | Alias `yt`; forces YouTube comment extraction. Accepts video URLs or IDs; no arguments displays its help. |
| `fetch-mix ai` | Aliases `providers`, `models`; defaults to provider list. |
| `fetch-mix ai list` | Alias `ls`; lists nine providers in auto-detection order. |
| `fetch-mix ai inspect [provider]` | Aliases `show`, `get`; accepts provider ID or display name case-insensitively. Missing provider displays help. |
| `fetch-mix dependencies` | Alias `deps`; defaults to verification. |
| `fetch-mix dependencies verify` | Aliases `check`, `status`; verifies fetch-track, yt-dlp, ffmpeg versions and fails on unsatisfied tools. |
| `fetch-mix dependencies install [dependency]...` | Aliases `add`, `get`; dependency is fetch-track, yt-dlp, or ffmpeg. Omitted names install missing tools. |
| `fetch-mix dependencies update [dependency]...` | Alias `upgrade`; omitted names update all three tools. |
| `fetch-mix upgrade` | Aliases `self-update`, `update-self`; atomically replaces the running executable from the explicit Rust release source, or reports current. Without configured source, fails as unpublished. |
| `fetch-mix skill` | Prints the canonical Skills CLI installation pointer verbatim. It does not read local guide files at runtime. |
| `fetch-mix help [COMMAND...]` | Displays root or nested command help; `fetch-mix help youtube`, `fetch-mix help ai`, `fetch-mix help ai list`, `fetch-mix help ai inspect`, `fetch-mix help dependencies`, `fetch-mix help dependencies verify`, `fetch-mix help dependencies install`, `fetch-mix help dependencies update`, `fetch-mix help upgrade`, `fetch-mix help skill`, and `fetch-mix help help` address the generated help tree. |
| `fetch-mix ai help [COMMAND]` | Group help; `fetch-mix ai help list`, `fetch-mix ai help inspect`, and `fetch-mix ai help help` display nested command help. |
| `fetch-mix dependencies help [COMMAND]` | Group help; `fetch-mix dependencies help verify`, `fetch-mix dependencies help install`, `fetch-mix dependencies help update`, and `fetch-mix dependencies help help` display nested command help. |

## Flags

Pipeline flags apply to root and youtube unless marked otherwise. Defaults below are runtime defaults when a flag is absent.

| Flag | Type and default | Contract |
| --- | --- | --- |
| `--out-dir`, `-o` | string path; unset | Default current directory plus sanitized mix title. |
| `--dry-run` | bool; false | Prints tracks and target paths, creates no audio/playlist/manifest; network access and cache writes may occur. |
| `--interactive`, `-i` | bool; false | Root only; human prompt selects 1..N, invalid/empty/EOF selects first. Agent mode selects first. |
| `--auto-install` | bool; false | Automatically installs missing dependencies before a real download. Otherwise human mode prompts; agent mode fails without prompting. |
| `--no-cache` | bool; false | Disables Mix page/comment/extraction caches. Does not change fetch-track's cache. |
| `--sources`, `-s` | string; youtube,soundcloud | Comma-separated youtube/soundcloud; passes through to fetch-track. |
| `--skip-verify` | bool; false | Passes through to skip audio-quality checks. |
| `--skip-metadata` | bool; false | Passes through to skip cover art/metadata. |
| `--llm-provider`, `-p` | string; auto | auto, ollama, litellm, gemini, openai, anthropic, openrouter, deepseek, groq, custom. |
| `--llm-model`, `-m` | string; empty | Overrides the selected provider's default model. |
| `--progress-target` | string; unset | stdout, stdout://, stderr, stderr://, unix://PATH, tcp://HOST:PORT, fd://N; Unix paths or host:port shorthand also accepted. Inherited descriptors/Unix sockets require Unix. |
| `--progress-socket` | string; unset | Fallback target when --progress-target is absent. |
| `--verbose`, `-v` | bool; false | Global; streams child downloader diagnostics. |
| `--version` | bool; false | Root only; raw version and newline. |
| `--help`, `-h` | bool; false | Displays aligned help. Agent help begins with a usage-skill alert. |

Progress target precedence: --progress-target, then --progress-socket, then FETCH_MIX_PROGRESS_TARGET. AGENT=1 enables machine formatting. Human paths and child diagnostics fold the home directory to `~`.

## Output and side effects

- Successful dry runs report status, target_dir, playlist, total_tracks and ordered track rows. Search results include numbered title lines and selected title. Output order follows extracted track order.
- Agent `ai list` emits four TSV fields: provider ID, default model, primary environment variable, status. `ai inspect` emits provider, name, default_model, env_var, status as key/value lines. First configured provider is WILL BE USED, other configured providers Detected, missing providers Not Detected (local defaults included).
- `dependencies verify` emits TSV name, detected version, minimum version, OK/FAIL. Installation/update emits installed/updated key/value lines. Upgrade emits status upgraded/current and version.
- Real downloads create target directory, numbered files (`01 - original.ext`), playlist.m3u, and mix_manifest.json. Playlist contains every planned track: completed files use their recorded name; incomplete tracks use an existing matching numbered file or a placeholder path. A playlist entry alone does not prove download success. Failed downloads remain recorded; a later run retries them. Existing unrelated numbered files are protected from overwrite.
- Manifest keys: mixTitle, targetDir, createdAt, updatedAt, tracks, skipped. Each track has index, artist, title, rawString, optional timestamp/actualFile, status, duration, bandwidthHz, qualityRating, gainOffsetDb, sourceUrl, error. Skipped items have optional timestamp, rawText, reason. Track identity/order must match; corrupt/mismatched manifests fail.
- Files are accepted from the child completion event path (expanding `~/`) or exactly one new regular file. Files must be nonempty and resolve within the target directory. Resume checks recorded file names and artifacts again.
- NDJSON progress is one event per line. Fields include timestamp, type, optional phase/message/error/step/total_steps/percent/track/candidate/result/summary. Unknown native child fields are preserved. Mix events include phase_start (search/scrape/youtube_comments), progress (parse), track_start, track_complete, complete. Child events retain track context.
- With stdout progress, stdout carries NDJSON only and human/status diagnostics use stderr. Other progress targets leave regular output on stdout. Malformed incoming child JSON lines are ignored; lines over 1 MiB fail. Shutdown accepts already queued socket connections and drains readers for up to 200 ms. Unix descriptor ownership and flags are restored after output ends.
- Mix caches use `.tmp/` when it exists or FETCH_MIX_DEV=1/true, otherwise platform cache directory/fetch-mix. Caches are lazy atomic JSON files; corrupted entries are misses. Native tools use the managed data directory followed by PATH.
- Downloads run sequentially with a short randomized delay. Unix cancellation kills the child process group, reaps the leader, and bounds draining inherited pipes. Initial SIGINT/SIGTERM cancels; a second signal forces termination.

## Exit statuses

| Status | Meaning |
| --- | --- |
| 0 | Success, help, version, or all requested downloads completed/resumed |
| 1 | Validation, I/O, network, provider, dependency, unpublished release, or partial download failure |
| 130 | Cancellation |

Errors are printed once to stderr. A command that downloaded some tracks can still fail; inspect the manifest before retrying. Output success does not prove the quality of a skipped verification step.
