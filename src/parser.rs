use crate::model::{SkippedItem, Track};
use regex::Regex;
use std::sync::LazyLock;

struct Patterns {
    header: Regex,
    end: Regex,
    links: Regex,
    brackets: Regex,
    label: Regex,
    play: Regex,
    timestamp: Regex,
    list: Regex,
    trim: Regex,
    whitespace: Regex,
    dash: Regex,
}
static RX: LazyLock<Patterns> = LazyLock::new(|| {
    let rx = |s| Regex::new(s).expect("checked parser expression");
    Patterns {
        header: rx(r"(?i)^(?:[=#]+\s*)?tracklist"),
        end: rx(r"(?i)^(?:---|===|related mixes|comments|similar sets|#+\s+[A-Za-z])"),
        links: rx(r"\\?\[([^\]]+)\\?\]\([^)]+\)"),
        brackets: rx(r"\\?[\[{].*?\\?[\]}]"),
        label: rx(r"(?i)^(?:intro\s*:\s*|outro\s*:\s*|track\s*\d+\s*:\s*)"),
        play: rx(r"▶\s*\d{1,2}:\d{2}\s*$"),
        timestamp: rx(r"^(?:\d{1,2}:\d{2}(?::\d{2})?|\[\d{1,2}:\d{2}(?::\d{2})?\])"),
        list: rx(r"^(?:(?:\d+[.\s\-]+|\*|-|#)\s*)+"),
        trim: rx(r#"^[\s_"*▶•–—\-:]+|[\s_"*▶•–—\-:]+$"#),
        whitespace: rx(r"\s+"),
        dash: rx(r"\s+[-‒–—―]\s+"),
    }
});
pub fn parse(content: &str) -> (Vec<Track>, Vec<SkippedItem>) {
    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    let section = content.lines().any(|l| RX.header.is_match(l.trim()));
    let mut inside = !section;
    for line in content.lines() {
        let mut text = line.trim().to_owned();
        if RX.header.is_match(&text) {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        // A single wiki list marker belongs to a track, not a section heading.
        if RX.end.is_match(&text) && !(text.starts_with("# ") && RX.dash.is_match(&text)) {
            if !tracks.is_empty() {
                break;
            }
            continue;
        }
        text = RX.list.replace(&text, "").into_owned();
        let timestamp = RX
            .timestamp
            .find(&text)
            .map(|m| m.as_str().trim_matches(['[', ']']).to_owned())
            .unwrap_or_default();
        text = RX.links.replace_all(&text, "$1").into_owned();
        text = RX.brackets.replace_all(&text, "").into_owned();
        text = RX.label.replace(&text, "").into_owned();
        text = RX.play.replace(&text, "").into_owned();
        text = RX.timestamp.replace(&text, "").into_owned();
        text = RX.list.replace(&text, "").into_owned();
        text = RX.trim.replace_all(&text, "").into_owned();
        text = RX.whitespace.replace_all(text.trim(), " ").into_owned();
        let parts: Vec<_> = RX.dash.split(&text).collect();
        if parts.len() < 2 {
            continue;
        }
        let artist = parts[0].trim();
        let title = parts[1..].join(" - ");
        let a = artist.to_lowercase();
        let t = title.to_lowercase();
        let placeholder = [&a, &t].iter().any(|s| {
            matches!(s.as_str(), "id" | "id-id")
                || s.contains("id - id")
                || s.contains("untitled - untitled")
        }) || a == "untitled" && t == "untitled";
        let reason = if matches!(a.as_str(), "intro" | "outro") {
            Some(format!("{a} marker"))
        } else if placeholder {
            Some("unidentified track placeholder".into())
        } else if artist.is_empty() || title.is_empty() {
            Some("incomplete track name".into())
        } else {
            None
        };
        if let Some(reason) = reason {
            skipped.push(SkippedItem {
                timestamp,
                raw_text: text,
                reason,
            });
        } else {
            tracks.push(Track::new(artist, &title, &timestamp));
        }
    }
    (tracks, skipped)
}
pub fn normalize_timestamp(input: &str, hours: bool) -> String {
    let parts: Option<Vec<u64>> = input
        .trim_matches(['[', ']'])
        .split(':')
        .map(|p| p.parse().ok())
        .collect();
    let Some(parts) = parts else {
        return input.into();
    };
    let (h, m, s) = match parts.as_slice() {
        [m, s] if *s < 60 => (m / 60, m % 60, *s),
        [h, m, s] if *m < 60 && *s < 60 => (*h, *m, *s),
        _ => return input.into(),
    };
    if hours || h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
pub fn format_duration(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
