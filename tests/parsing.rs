use fetch_mix_cli::{cache::Cache, parser};

#[test]
fn real_tracklist_formats_and_skips() {
    let (tracks, skipped) = parser::parse(
        "Ignored - Header\n== Tracklist ==\n# [00:00] Intro - Welcome\n# [00:31] Bicep - Glue [Label]\n* 02. Artist – Another – Remix\n03. ID - ID\n04. Untitled - Untitled\n=== Related mixes\nOther - Ignore",
    );
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].artist, "Bicep");
    assert_eq!(tracks[0].timestamp, "00:31");
    assert_eq!(tracks[1].title, "Another - Remix");
    assert_eq!(skipped.len(), 3);
    let (tracks, _) = parser::parse(
        "1. [Artist](https://example.com) - [Title](https://example.org)\n02:30 Other - Tune\nNo separator",
    );
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].artist, "Artist");
    assert_eq!(tracks[0].title, "Title");
    assert_eq!(tracks[1].timestamp, "02:30");
    assert_eq!(parser::normalize_timestamp("05:30", true), "00:05:30");
    assert_eq!(parser::normalize_timestamp("bad", true), "bad");
    assert_eq!(parser::format_duration(3661.8), "1:01:01");
    assert_eq!(parser::format_duration(61.0), "1:01");
}

#[test]
fn cache_round_trip_corruption_atomicity_and_safe_names() {
    let dir = tempfile::tempdir_in(".tmp").unwrap();
    let cache = Cache::new(dir.path().join("cache"));
    assert!(!cache.dir().exists());
    assert!(cache.get::<Vec<String>>("tracks.json").is_none());
    cache.put("tracks.json", &vec!["A - B"]).unwrap();
    assert_eq!(cache.get::<Vec<String>>("tracks.json").unwrap(), ["A - B"]);
    std::fs::write(cache.dir().join("broken.json"), "{").unwrap();
    assert!(cache.get::<String>("broken.json").is_none());
    assert!(cache.put("../outside", &42).is_err());
    cache.delete("tracks.json").unwrap();
    cache.delete("tracks.json").unwrap();
    assert_eq!(Cache::hash("set"), Cache::hash("set"));
    assert_ne!(Cache::hash("set"), Cache::hash("other"));
}
