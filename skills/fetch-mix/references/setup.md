---
created_on: 2026-10-14 08:47
last_modified: 2026-10-14 08:47
status: current
---

# Setup

## External tools

Mix verifies Rust fetch-track >=0.1.0, yt-dlp >=2024.08.01, and ffmpeg >=4.4. Fetch-track additionally requires ffprobe >=4.4 for probing audio. YouTube comment extraction passes `--js-runtimes node` to yt-dlp, so Node must be installed. Managed executables are resolved before PATH from `$XDG_DATA_HOME/fetch-mix/bin`, default `~/.local/share/fetch-mix/bin` (LOCALAPPDATA fallback on Windows).

`fetch-mix dependencies install` uses native release installation for fetch-track, yt-dlp release assets, and the operating system's package manager for ffmpeg. Updates use the corresponding native tool updater/package manager. Install/update operations may require privileges. Dry runs skip dependency verification, but uncached YouTube extraction still invokes yt-dlp.

## AI providers

Auto selection takes the first provider with a nonempty configuration variable in this order. Explicit `--llm-model` overrides every model below. OLLAMA_MODEL and LITELLM_MODEL override only their respective defaults. Values are inspected for presence; provider inspection does not make an API request or prove credentials work.

| ID / display name | Default model | Configuration |
| --- | --- | --- |
| ollama / Ollama (Local) | llama3.2 | OLLAMA_HOST, then OLLAMA_URL; default http://localhost:11434; optional OLLAMA_MODEL |
| litellm / LiteLLM (Proxy) | gemini-2.5-flash | LITELLM_BASE_URL, LITELLM_URL, LITELLM_API_BASE; default http://localhost:4000; optional LITELLM_API_KEY or LITELLM_KEY, LITELLM_MODEL |
| gemini / Google Gemini | gemini-2.5-flash | GEMINI_API_KEY |
| openai / OpenAI | gpt-4o-mini | OPENAI_API_KEY |
| anthropic / Anthropic Claude | claude-3-5-haiku-latest | ANTHROPIC_API_KEY |
| openrouter / OpenRouter | google/gemini-2.5-flash | OPENROUTER_API_KEY |
| deepseek / DeepSeek | deepseek-chat | DEEPSEEK_API_KEY |
| groq / Groq | llama-3.3-70b-versatile | GROQ_API_KEY |
| custom / Custom OpenAI-Compat | gpt-4o-mini | OPENAI_BASE_URL; optional OPENAI_API_KEY |

Local endpoints require HTTP(S). Explicit local provider selection can use its default endpoint even when auto-detection finds no configuration. Web extraction can use deterministic parsing without AI. YouTube comments fetch at most 200 top-level comments, rank candidate tracklists by likes, and use a structured AI response to choose/extract tracks; cached extraction avoids that call.

## Rust release sources

Rust artifacts are unpublished by default. Existing canonical Go archives are not used for Rust installation or upgrades. Configure a trusted GitHub-compatible release service explicitly, with all three variables per source:

| Purpose | Variables |
| --- | --- |
| Install/update Rust fetch-track | FETCH_TRACK_RUST_RELEASE_BASE_URL, FETCH_TRACK_RUST_RELEASE_OWNER, FETCH_TRACK_RUST_RELEASE_REPO |
| Update this executable | FETCH_MIX_RUST_RELEASE_BASE_URL, FETCH_MIX_RUST_RELEASE_OWNER, FETCH_MIX_RUST_RELEASE_REPO |

BASE_URL requires HTTPS; loopback HTTP is accepted for local testing. URL credentials, query, and fragment are rejected. OWNER and REPO are single ASCII alphanumeric/period/underscore/hyphen names other than `.` or `..`. Partial configuration fails validation.

Native release discovery uses `/OWNER/REPO/releases/latest` redirect tags. Downloads use `/OWNER/REPO/releases/download/TAG/PREFIX_VERSION_OS_ARCH.tar.gz` (zip on Windows), trying repository prefix then binary prefix. The archive must contain the matching regular executable. Installation stages and replaces the executable atomically. Self-update skips current or older versions. Unconfigured Rust installation or self-upgrade returns an unpublished error instead of downloading Go artifacts.

## Environment and files

- AGENT=1 enables machine output; FETCH_MIX_PROGRESS_TARGET supplies the fallback NDJSON target.
- FETCH_MIX_DEV=1 or true selects the current `.tmp/` cache directory; an existing `.tmp/` also selects it.
- HOME and platform directory settings determine home-folded display paths; XDG_CACHE_HOME/platform cache determines the cache fallback. XDG_DATA_HOME determines the managed tools directory.
- PATH supplies external tools after managed executables. AI API keys are inherited by native provider clients and remain absent from metadata output.
