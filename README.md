Discover a DJ set's tracklist and turn it into a numbered, resumable audio playlist. This Rust implementation provides `fetch-mix` for people and automation.

# What It Does

- **Discover sets:** Search MixesDB and mirror sites, or read a tracklist URL directly.
- **Extract tracks:** Parse tracklists locally or use a configured AI provider, including YouTube comment extraction.
- **Download playlists:** Run the Rust `fetch-track` downloader, number verified artifacts, and save an M3U playlist and resume manifest.
- **Report progress:** Stream NDJSON to stdout, stderr, Unix/TCP sockets, or inherited Unix descriptors.
- **Manage tools:** Verify versions, install missing dependencies, and update from explicitly configured release sources.

# How It Works

- Give a query, tracklist URL, or YouTube URL. Queries select the first result; `--interactive` offers a human selection prompt.
- Preview with `--dry-run`, then download into the mix-title directory or `--out-dir`.
- Run the same tracklist into the same directory to resume completed, nonempty artifacts.

# How it Really Works

- Web discovery queries MixesDB and mirror sites; direct URLs read the supplied page. Optional AI extraction sends page content or YouTube comments to the configured provider, whose usage may incur charges.
- Every track runs as a separate `fetch-track` process with streamed progress. Cancellation terminates its process group on Unix.
- Atomic JSON manifests record complete track identity, file names, quality metrics, and failures; changed tracklists require another directory.
- Cached page and extraction results live in an existing `.tmp/` or the platform cache directory. Repeat runs reuse complete artifacts and retry failed tracks; `--no-cache` disables Mix caches.
- `AGENT=1` selects machine formatting. Stdout progress contains NDJSON, with diagnostics on stderr. Success exits 0, failures exit 1, and cancellation exits 130.
- External tool installers can change managed binaries or operating-system packages; self-update requires an explicit Rust release source.

# Installation

Rust release artifacts are unpublished. This independent repository currently provides a development build; build instructions are in [AGENTS.md](AGENTS.md). Existing Go release archives contain the Go implementation.

At runtime use Rust `fetch-track` 0.1.0+, yt-dlp 2024.08.01+, and ffmpeg 4.4+. The track downloader additionally needs ffprobe 4.4+. YouTube comment extraction invokes yt-dlp with Node as its JavaScript runtime. `fetch-mix dependencies verify` checks Mix's three managed dependencies.

# Setup

- Web tracklists work with the local parser. YouTube comments require a configured AI provider unless extraction is cached. Use `fetch-mix ai list` and `fetch-mix ai inspect <provider>` to inspect selection without printing secrets.
- Install [yt-dlp](https://github.com/yt-dlp/yt-dlp#installation), [ffmpeg/ffprobe](https://ffmpeg.org/download.html), and the sibling Rust track downloader for audio downloads. Install [Node](https://nodejs.org/en/download) for YouTube comment extraction.
- Provider configuration and explicit Rust release sources are documented in the [usage setup reference](skills/fetch-mix/references/setup.md).

# Quick Start

```sh
fetch-mix --dry-run "DJ set artist date"
fetch-mix --out-dir ./my-mix "DJ set artist date"
AGENT=1 fetch-mix --progress-target stdout --out-dir ./my-mix "DJ set artist date"
fetch-mix youtube --dry-run "https://www.youtube.com/watch?v=VIDEO_ID"
fetch-mix dependencies verify
AGENT=1 fetch-mix ai inspect ollama
```

Sample Output (Ollama configuration absent):

```text
provider: ollama
name: Ollama (Local)
default_model: llama3.2
env_var: OLLAMA_HOST
status: Not Detected (default: http://localhost:11434)
```

Agent metadata output uses TSV or `key: value` lines. Progress output is one JSON event per line; when targeting stdout, diagnostics move to stderr. A dry run reports `status: dry_run`, target directory, playlist path, total tracks, and track rows without creating playlist/audio/manifest files. Discovery caches may still be written.

# Options & Flags

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--verbose` | `-v` | false | Global; show downloader diagnostics |
| `--help` | `-h` | false | Show aligned command help |
| `--version` | | false | Root only; print raw version |
| `--interactive` | `-i` | false | Root only; choose a matching set in human mode |

`fetch-mix` and `fetch-mix youtube`:

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--out-dir <path>` | `-o` | mix-title directory | Output directory |
| `--dry-run` | | false | Preview without audio writes |
| `--auto-install` | | false | Install missing tools without prompting |
| `--no-cache` | | false | Disable Mix discovery/extraction caching |
| `--sources <string>` | `-s` | youtube,soundcloud | Track search sources |
| `--skip-verify` | | false | Skip track audio-quality checks |
| `--skip-metadata` | | false | Skip cover art and tagging |
| `--llm-provider <string>` | `-p` | auto | Select AI provider |
| `--llm-model <string>` | `-m` | empty | Override provider model |
| `--progress-target <string>` | | unset | NDJSON destination |
| `--progress-socket <string>` | | unset | Fallback progress destination |

The [complete usage skill](skills/fetch-mix/SKILL.md) covers commands, aliases, output schemas, side effects, and environment precedence. `fetch-mix skill` prints the canonical installation pointer; that pointer does not establish that this unpublished Rust guide is available remotely. `--no-cache` leaves the separate downloader's cache policy unchanged.

Failed tracks are retained in the manifest and make the command fail. Resume requires identical track identity/order and nonempty regular artifacts. Local AI proxy aliases participate in auto-detection; blocked Unix progress descriptors remain cancellable.

# License

[MIT](LICENSE), Copyright © 2026 Alex Gorbatchev.
