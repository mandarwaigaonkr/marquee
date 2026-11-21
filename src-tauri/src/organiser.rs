//! Turns file paths into a proposed library structure by reading their names.
//! Pure functions only: nothing here touches the database.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub const VIDEO_EXTENSIONS: [&str; 13] = [
    "mkv", "mp4", "avi", "mov", "webm", "m4v", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg", "ogm",
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Proposed {
    pub path: String,
    /// "episode", "movie" or "unsorted" (no pattern found; left out unless the user opts in).
    pub kind: String,
    pub series: String,
    pub season: i64,
    pub season_title: Option<String>,
    pub episode: i64,
    pub episode_end: Option<i64>,
    pub title: String,
    pub year: Option<i64>,
    pub include: bool,
    /// For a special: the season it airs after, when the name or folder says so
    /// ("Show S2 OVA", "Season 02/Show - OVA.mkv"). None leaves it at the end.
    #[serde(default)]
    pub after_season: Option<i64>,
}

static SXXEXX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bS(\d{1,2})[ ._-]*E(\d{1,3})(?:[ ._-]*E(\d{1,3}))?").unwrap());
static NXNN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[^0-9a-z])(\d{1,2})x(\d{2,3})(?:[^0-9]|$)").unwrap());
static EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:episode|ep)[ ._-]*(\d{1,4})\b").unwrap());
static ANIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r" - (\d{2,4})(?:v\d)?(?:[ ._\[(]|$)").unwrap());
/// "Show S2 - 05": a season tag right before the dash.
static ANIME_SEASON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:S|Season )(\d{1,2})\s*$").unwrap());
/// "sample" as the whole name or as a release-style suffix ("show.s01e01-sample").
/// As a word in a title ("7. Sample Questions", "Sample This") it means nothing.
static SAMPLE_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:^|[._-])sample$").unwrap());
static SAMPLE_DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:samples?|trailers?)$").unwrap());
static NUMBERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{1,3})[.) _-]+(\S.*)$").unwrap());
static SEASON_DIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:season|series|s)[ ._-]*(\d{1,2})$").unwrap());
/// Folders that hold a series' specials, as Plex, Jellyfin and Kodi name them ("Season 00" is SEASON_DIR).
static SPECIALS_DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:specials?|ovas?|sps?)$").unwrap());
/// Bonus-material folders: Plex's local extras names plus the usual anime disc ones.
/// What is in them is offered as a special of the series above, but left unticked.
static EXTRAS_DIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:extras?|bonus(?: features)?|featurettes?|behind the scenes|deleted scenes|interviews?|shorts|creditless|nc|ncop|nced)$").unwrap()
});
/// Special markers that are never ordinary words: a whole word anywhere, with an optional number.
/// Kodi's anime rules group the same ones into season 0.
static SPECIAL_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[\s(-])(OVA|OAV|OAD|SP|Picture Drama)(?:(?: - | ?)(\d{1,3}))?(?:v\d)?(?:$|[\s)-])").unwrap());
/// "Special" is an ordinary word ("Special Forces", "Special Training"), so it only counts
/// after other words and last in the name, or before a number, a closing bracket or a dash.
static SPECIAL_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s(Specials?)(?:(?: - | ?)(\d{1,3}))?(?:$|\s*\)|\s+-\s)").unwrap());
/// Creditless openings and endings from anime disc releases: extras, offered unticked.
static CREDITLESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[\s(-])(NCOP|NCED|Creditless (?:Opening|Ending|OP|ED))(?: ?(\d{1,2}))?(?:v\d)?(?:$|[\s)-])").unwrap());
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(19\d{2}|20\d{2})\b").unwrap());
static PAREN_YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((?:19|20)\d{2}\)").unwrap());
static TRAILING_YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\(?((?:19|20)\d{2})\)?$").unwrap());
static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\]]*\]").unwrap());
/// Release tags that are never part of a real name.
static TAGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(2160p|1080p|1080i|720p|480p|4k|uhd|x264|x265|h ?264|h ?265|hevc|10bit|web[ -]?dl|webrip|bluray|blu-ray|brrip|bdrip|dvdrip|hdtv|hdr10\+?|remux|s\d{1,2}(?: complete)?|ddp?5 ?1|season \d+-\d+)\b",
    )
    .unwrap()
});
/// Tags that are also ordinary words ("Extended Family", "The Proper Way"), so they
/// only count when written in capitals, the way release names write them.
static SHOUTED_TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(AVC|AV1|HDR|DV|DoVi|DOVI|PROPER|REPACK|EXTENDED|DSNP|AMZN|NF|ATMOS|Atmos|AAC)\b").unwrap());
/// The same words in any case, which only count directly before a real tag
/// ("s02e01.proper.1080p", "2001.Extended.1080p").
static TAG_WORD_AT_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|\s)(?:avc|av1|hdr|dv|dovi|proper|repack|extended|dsnp|amzn|nf|atmos|aac)$").unwrap());

/// The latest year a file could have been released, so the number in
/// "Blade Runner 2049" is not mistaken for one.
fn latest_release_year() -> i64 {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    1971 + (seconds / 31_556_952) as i64
}

/// The release year in a name: the last plausible one ("2012 (2009)" was released in 2009).
fn release_year(text: &str) -> Option<i64> {
    let latest = latest_release_year();
    YEAR.captures_iter(text).filter_map(|c| c[1].parse::<i64>().ok()).filter(|y| *y <= latest).last()
}

/// Whether a name carries a year the way films do: in brackets, or straight before
/// a release tag ("12.Angry.Men.1957.1080p"). "5. Windows Server 2019 Setup" does not.
fn has_film_year(stem: &str) -> bool {
    if PAREN_YEAR.is_match(stem) {
        return true;
    }
    let spaced = stem.replace(['.', '_'], " ");
    let latest = latest_release_year();
    YEAR.captures_iter(&spaced).any(|c| {
        let rest = spaced[c.get(0).unwrap().end()..].trim_start_matches(|ch: char| ch.is_whitespace() || "-([".contains(ch));
        let tag_next = [TAGS.find(rest), SHOUTED_TAGS.find(rest)].into_iter().flatten().any(|m| m.start() == 0);
        tag_next && c[1].parse::<i64>().is_ok_and(|y| y <= latest)
    })
}

/// Name cleaned for display: separators to spaces, release tags and brackets removed.
pub fn clean(raw: &str) -> String {
    let no_brackets = BRACKETS.replace_all(raw, " ");
    let spaced = no_brackets.replace(['.', '_'], " ");
    let first_tag = [TAGS.find(&spaced), SHOUTED_TAGS.find(&spaced)].into_iter().flatten().map(|m| m.start()).min();
    let cut = first_tag.map_or(spaced.as_str(), |at| &spaced[..at]);
    let edges = |c: char| c.is_whitespace() || "-–([{,".contains(c);
    let mut trimmed = cut.trim_matches(edges);
    if first_tag.is_some() {
        while let Some(m) = TAG_WORD_AT_END.find(trimmed) {
            trimmed = trimmed[..m.start()].trim_matches(edges);
        }
    }
    // A trailing "(2011" or "(2011)" is a release year, not part of the name.
    let without_year = match TRAILING_YEAR.captures(trimmed) {
        Some(c) if c[1].parse::<i64>().is_ok_and(|y| y <= latest_release_year()) => trimmed[..c.get(0).unwrap().start()].trim_matches(edges),
        _ => trimmed,
    };
    let name = if without_year.is_empty() { trimmed } else { without_year };
    let words = name.split_whitespace();
    // Names typed entirely in lower case ("the.bear") read better capitalised.
    if name.chars().any(char::is_uppercase) {
        return words.collect::<Vec<_>>().join(" ");
    }
    words
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Key used to recognise the same series across imports, whatever the punctuation.
pub fn match_key(title: &str) -> String {
    title.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn dir_name(path: &Path, levels_up: usize) -> String {
    path.ancestors()
        .nth(levels_up)
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The series and season a folder names: `levels_up` is the folder, and when it is a
/// season or specials folder the series is the one above it.
fn series_at(path: &Path, levels_up: usize) -> (String, Option<i64>) {
    let dir = dir_name(path, levels_up);
    if SPECIALS_DIR.is_match(dir.trim()) {
        return (clean(&dir_name(path, levels_up + 1)), Some(0));
    }
    match SEASON_DIR.captures(dir.trim()) {
        Some(c) => (clean(&dir_name(path, levels_up + 1)), c[1].parse().ok()),
        None => (clean(&dir), None),
    }
}

/// The folder that names the series: the parent, or its parent when the file sits in a
/// season folder. A specials folder counts as season 0.
fn series_from_dirs(path: &Path) -> (String, Option<i64>) {
    series_at(path, 1)
}

fn episode(path: &Path, series: String, season: i64, ep: i64, end: Option<i64>, rest: &str) -> Proposed {
    let title = clean(rest);
    Proposed {
        path: path.to_string_lossy().into_owned(),
        kind: "episode".into(),
        series,
        season,
        season_title: None,
        episode: ep,
        episode_end: end.filter(|e| *e > ep),
        title: if title.is_empty() { format!("Episode {ep}") } else { title },
        year: None,
        include: true,
        after_season: None,
    }
}

/// A special in season 0. `number` 0 means the name gave none; `scan` numbers those.
fn special(path: &Path, series: String, number: i64, title: String, after_season: Option<i64>) -> Proposed {
    Proposed {
        path: path.to_string_lossy().into_owned(),
        kind: "episode".into(),
        series,
        season: 0,
        season_title: None,
        episode: number,
        episode_end: None,
        title,
        year: None,
        include: true,
        after_season: after_season.filter(|s| *s > 0),
    }
}

/// The name for a special with no title of its own: "OVA 2", "Special".
fn special_label(tag: &str, number: Option<i64>) -> String {
    let base = match tag.to_lowercase().as_str() {
        "ova" | "oav" => "OVA",
        "oad" => "OAD",
        "picture drama" => "Picture drama",
        "ncop" | "creditless opening" | "creditless op" => "Creditless opening",
        "nced" | "creditless ending" | "creditless ed" => "Creditless ending",
        _ => "Special",
    };
    number.map_or(base.to_string(), |n| format!("{base} {n}"))
}

/// File name with brackets dropped and separators as single spaces.
fn spaced_name(stem: &str) -> String {
    BRACKETS.replace_all(stem, " ").replace(['.', '_'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reads "Show - OVA 2 - The Beach", "Show S2 SP01", "Doctor Who - The Runaway Bride (Christmas Special)".
fn parse_special(path: &Path, stem: &str, dir_series: &str, dir_season: Option<i64>) -> Option<Proposed> {
    let spaced = spaced_name(stem);
    let (marker, tag, number, extra) = [(&*CREDITLESS, true), (&*SPECIAL_TAG, false), (&*SPECIAL_WORD, false)].iter().find_map(|(re, extra)| {
        let c = re.captures(&spaced)?;
        let n = c.get(2).and_then(|n| n.as_str().parse::<i64>().ok());
        Some((c.get(1).unwrap(), c[1].to_string(), n, *extra))
    })?;
    let edges = |c: char| c.is_whitespace() || c == '-';
    let mut before = spaced[..marker.start()].trim_end_matches(edges);
    // Inside brackets ("(Christmas Special)") the words before the bracket are the title.
    if let Some(open) = before.rfind('(').filter(|open| before.rfind(')').is_none_or(|close| close < *open)) {
        before = before[..open].trim_end_matches(edges);
    }
    // "Show S2 OVA": the season tag says where it airs and is not part of the name.
    let mut tag_season = None;
    if let Some(c) = ANIME_SEASON.captures(before) {
        tag_season = c[1].parse::<i64>().ok();
        before = before[..c.get(0).unwrap().start()].trim_end_matches(edges);
    }
    let (name, title_part) = match before.find(" - ") {
        Some(dash) => (&before[..dash], before[dash + 3..].trim()),
        None => (before, ""),
    };
    let series = Some(clean(name)).filter(|s| !s.is_empty()).unwrap_or_else(|| dir_series.to_string());
    if series.is_empty() {
        return None;
    }
    let rest = spaced[marker.end()..].trim_start_matches(|c: char| c.is_ascii_digit() || edges(c) || c == ')');
    let rest = rest.strip_prefix('v').filter(|r| r.starts_with(|c: char| c.is_ascii_digit())).map_or(rest, |r| r.trim_start_matches(|c: char| c.is_ascii_digit() || edges(c)));
    let after = Some(clean(rest)).filter(|t| t.chars().any(char::is_alphabetic));
    let title = after.or(Some(clean(title_part)).filter(|t| !t.is_empty())).unwrap_or_else(|| special_label(&tag, number));
    let mut found = special(path, series, number.unwrap_or(0), title, tag_season.or(dir_season));
    found.include = !extra;
    Some(found)
}

/// `numbered_ok` allows the "12. Lesson name" pattern, which is only trusted when
/// several files in the same folder share it (otherwise "12 Angry Men" would match).
pub fn parse(path: &Path, numbered_ok: bool) -> Proposed {
    let mut proposed = parse_name(path, numbered_ok);
    // Samples and trailers are listed but left unticked.
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if SAMPLE_FILE.is_match(&stem) || SAMPLE_DIR.is_match(dir_name(path, 1).trim()) {
        proposed.include = false;
    }
    proposed
}

fn parse_name(path: &Path, numbered_ok: bool) -> Proposed {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // Bonus material ("Show/Extras/Making Of.mkv") is a special of the series above, unticked.
    if EXTRAS_DIR.is_match(dir_name(path, 1).trim()) {
        let (series, season) = series_at(path, 2);
        if !series.is_empty() {
            let title = clean(&stem);
            let mut extra = parse_special(path, &stem, &series, season)
                .unwrap_or_else(|| special(path, series.clone(), 0, if title.is_empty() { stem.clone() } else { title }, season));
            extra.series = series;
            extra.include = false;
            return extra;
        }
    }
    let (dir_series, dir_season) = series_from_dirs(path);
    let named = |before: &str| {
        let s = clean(before);
        if s.is_empty() { dir_series.clone() } else { s }
    };

    if let Some(c) = SXXEXX.captures(&stem) {
        let m = c.get(0).unwrap();
        let end = c.get(3).and_then(|e| e.as_str().parse().ok());
        return episode(path, named(&stem[..m.start()]), c[1].parse().unwrap(), c[2].parse().unwrap(), end, &stem[m.end()..]);
    }
    if let Some(c) = NXNN.captures(&stem) {
        let (s, e) = (c.get(1).unwrap(), c.get(2).unwrap());
        return episode(path, named(&stem[..s.start()]), c[1].parse().unwrap(), c[2].parse().unwrap(), None, &stem[e.end()..]);
    }
    // Specials come before the looser patterns: "Show OVA - 02" is OVA 2, not episode 2.
    // A special in a season folder airs after that season.
    if let Some(found) = parse_special(path, &stem, &dir_series, dir_season) {
        return found;
    }
    if let Some(c) = EPISODE.captures(&stem) {
        let m = c.get(0).unwrap();
        return episode(path, named(&stem[..m.start()]), dir_season.unwrap_or(1), c[1].parse().unwrap(), None, &stem[m.end()..]);
    }
    // "Name - 2019" is a year, not episode 2019; in "Name - 2005 - 01" the episode comes after it.
    let anime = {
        let mut at = 0;
        loop {
            match ANIME.captures_at(&stem, at) {
                Some(c) if YEAR.is_match(&c[1]) => at = c.get(1).unwrap().end(),
                other => break other,
            }
        }
    };
    if let Some(c) = anime {
        let before = BRACKETS.replace_all(&stem[..c.get(0).unwrap().start()], " ");
        let before = before.trim();
        // "Show Season 2 - 05": the season tag is not part of the series name.
        let tag = ANIME_SEASON.captures(before);
        let name = tag.as_ref().map_or(before, |s| &before[..s.get(0).unwrap().start()]);
        let season = tag.as_ref().and_then(|s| s[1].parse().ok()).or(dir_season).unwrap_or(1);
        return episode(path, named(name), season, c[1].parse().unwrap(), None, "");
    }
    // A name with a film-style year is a film ("12 Angry Men (1957)"), however it starts.
    // Inside a numbered section folder it is a lesson regardless.
    let parent = dir_name(path, 1);
    if numbered_ok && (NUMBERED.is_match(&parent) || !has_film_year(&stem)) {
        if let Some(c) = NUMBERED.captures(&stem) {
            // "3. Protocols/1. ARP.mp4": the numbered parent folder is the season.
            let (series, season, season_title) = match NUMBERED.captures(&parent) {
                Some(p) => (clean(&dir_name(path, 2)), p[1].parse().unwrap_or(1), Some(clean(&p[2]))),
                None => (dir_series.clone(), dir_season.unwrap_or(1), None),
            };
            let mut ep = episode(path, series, season, c[1].parse().unwrap(), None, &c[2]);
            ep.season_title = season_title;
            return ep;
        }
    }

    // Anything else in a specials folder is a special of the series above ("Specials/The Making Of.mkv").
    if dir_season == Some(0) && !dir_series.is_empty() {
        // "Dark - Making Of" in Dark's specials: the series name is not part of the title.
        let title = clean(&stem);
        let title = title
            .get(dir_series.len()..)
            .filter(|_| match_key(title.get(..dir_series.len()).unwrap_or_default()) == match_key(&dir_series))
            .map(|rest| rest.trim_start_matches(|c: char| c.is_whitespace() || c == '-').to_string())
            .filter(|rest| !rest.is_empty())
            .unwrap_or(title);
        return special(path, dir_series, 0, if title.is_empty() { stem } else { title }, None);
    }
    let year = release_year(&stem);
    let title = clean(&stem);
    Proposed {
        path: path.to_string_lossy().into_owned(),
        kind: if year.is_some() { "movie" } else { "unsorted" }.into(),
        series: String::new(),
        season: 0,
        season_title: None,
        episode: 0,
        episode_end: None,
        title: if title.is_empty() { stem } else { title },
        year,
        include: year.is_some(),
        after_season: None,
    }
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                collect(&entry.path(), out);
            }
        }
    } else if is_video(path) {
        out.push(path.to_path_buf());
    }
}

/// Every video file in the given files and folders.
pub fn video_files(paths: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for p in paths {
        collect(Path::new(p), &mut files);
    }
    files.sort();
    files.dedup();
    files
}

/// Gives specials whose names carry no number the next free numbers in their series,
/// in file order, so "OVA" and "OVA - The Beach" don't both become special 0.
fn number_specials(proposed: &mut [Proposed]) {
    let is_special = |p: &Proposed| p.kind == "episode" && p.season == 0;
    let mut top: HashMap<String, i64> = HashMap::new();
    for p in proposed.iter().filter(|p| is_special(p)) {
        let n = top.entry(match_key(&p.series)).or_default();
        *n = (*n).max(p.episode);
    }
    for p in proposed.iter_mut().filter(|p| is_special(p) && p.episode == 0) {
        let n = top.entry(match_key(&p.series)).or_default();
        *n += 1;
        p.episode = *n;
    }
}

/// Expands folders, keeps video files, and proposes where each one belongs.
pub fn scan(paths: &[String]) -> Vec<Proposed> {
    let files = video_files(paths);

    let mut numbered_per_dir: HashMap<PathBuf, usize> = HashMap::new();
    for f in &files {
        let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if NUMBERED.is_match(&stem) {
            *numbered_per_dir.entry(f.parent().unwrap_or(f).to_path_buf()).or_default() += 1;
        }
    }

    let mut proposed: Vec<Proposed> = files
        .iter()
        .map(|f| {
            let dir = f.parent().unwrap_or(f);
            let siblings = numbered_per_dir.get(dir).copied().unwrap_or(0);
            // A lone "1. Intro.mp4" still counts when its folder is numbered too ("1. Course Introduction").
            let in_numbered_dir = dir.file_name().is_some_and(|d| NUMBERED.is_match(&d.to_string_lossy()));
            parse(f, siblings >= 2 || in_numbered_dir)
        })
        .collect();
    number_specials(&mut proposed);
    proposed.sort_by(|a, b| {
        (a.kind != "episode", a.series.to_lowercase(), a.season, a.episode, &a.title)
            .cmp(&(b.kind != "episode", b.series.to_lowercase(), b.season, b.episode, &b.title))
    });
    proposed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_is_a_word_in_titles_but_clutter_after_a_season_tag() {
        assert_eq!(clean("[TutsNode.net] - The Complete IP Networks Course From Zero to Expert!"), "The Complete IP Networks Course From Zero to Expert!");
        assert_eq!(clean("WandaVision.S01.COMPLETE.HDR.2160p.WEB-DL"), "WandaVision");
    }

    #[test]
    fn tag_words_inside_a_name_are_kept_unless_shouted() {
        assert_eq!(ep(r"C:\Downloads\Extended.Family.S01E01.720p.mkv").0, "Extended Family");
        assert_eq!(ep(r"C:\Downloads\The.Proper.Way.S01E01.mkv").0, "The Proper Way");
        assert_eq!(ep(r"C:\x\Show.S01E02.Pilot.REPACK.720p.mkv").4, "Pilot");
    }

    #[test]
    fn numbered_names_with_a_year_are_films_not_lessons() {
        let film = parse(Path::new(r"D:\Movies\12 Angry Men (1957).mkv"), true);
        assert_eq!((film.kind.as_str(), film.title.as_str(), film.year), ("movie", "12 Angry Men", Some(1957)));
    }

    #[test]
    fn numbered_lessons_that_mention_a_year_stay_in_their_course() {
        let setup = parse(Path::new(r"D:\Course\Server Setup\5. Windows Server 2019 Setup.mp4"), true);
        assert_eq!((setup.kind.as_str(), setup.episode, setup.title.as_str()), ("episode", 5, "Windows Server 2019 Setup"));
        let news = parse(Path::new(r"D:\Course\Server Setup\6. What's New in 2023.mp4"), true);
        assert_eq!((news.kind.as_str(), news.episode), ("episode", 6));
        let in_section = parse(Path::new(r"D:\Course\3. Server Setup\2. Overview (2021).mp4"), true);
        assert_eq!((in_section.kind.as_str(), in_section.season, in_section.episode), ("episode", 3, 2));
        let film = parse(Path::new(r"D:\Movies\12.Angry.Men.1957.1080p.BluRay.mkv"), true);
        assert_eq!((film.kind.as_str(), film.title.as_str(), film.year), ("movie", "12 Angry Men", Some(1957)));
    }

    #[test]
    fn release_words_in_any_case_are_dropped_right_before_a_tag() {
        assert_eq!(ep(r"C:\x\the.bear.s02e01.proper.1080p.web.h264-cakes.mkv").4, "Episode 1");
        assert_eq!(ep(r"C:\x\show.s01e01.hdr.2160p.mkv").4, "Episode 1");
        let film = parse(Path::new(r"C:\x\The.Fellowship.of.the.Ring.2001.Extended.1080p.BluRay.mkv"), false);
        assert_eq!((film.title.as_str(), film.year), ("The Fellowship of the Ring", Some(2001)));
    }

    #[test]
    fn a_year_before_the_episode_number_is_skipped() {
        assert_eq!(ep(r"C:\x\Doctor Who - 2005 - 01 - Rose.mkv"), ("Doctor Who".into(), 1, 1, None, "Episode 1".into()));
        assert_eq!(ep(r"C:\x\[Group] Fruits Basket - 2019 - 05 [1080p].mkv"), ("Fruits Basket".into(), 1, 5, None, "Episode 5".into()));
    }

    #[test]
    fn a_spelled_out_season_before_the_dash_is_not_part_of_the_series_name() {
        assert_eq!(ep(r"C:\x\[Group] Mushoku Tensei Season 2 - 05 [1080p].mkv"), ("Mushoku Tensei".into(), 2, 5, None, "Episode 5".into()));
    }

    #[test]
    fn sample_as_a_title_word_is_still_ticked() {
        assert!(parse(Path::new(r"D:\Course\4. Subnetting\7. Sample Questions.mp4"), true).include);
        assert!(parse(Path::new(r"C:\x\Sample.This.2012.1080p.mkv"), false).include);
        assert!(parse(Path::new(r"C:\x\Show.S02E04.Free.Sample.720p.mkv"), false).include);
        assert!(!parse(Path::new(r"C:\x\show.s01e01-sample.mkv"), false).include);
        assert!(!parse(Path::new(r"C:\x\sample.mkv"), false).include);
    }

    #[test]
    fn a_film_takes_its_last_plausible_year() {
        let sequel = parse(Path::new(r"C:\x\Blade Runner 2049 (2017).mkv"), false);
        assert_eq!((sequel.title.as_str(), sequel.year), ("Blade Runner 2049", Some(2017)));
        let numeric = parse(Path::new(r"C:\x\1917.2019.1080p.BluRay.mkv"), false);
        assert_eq!((numeric.title.as_str(), numeric.year), ("1917", Some(2019)));
        let no_year = parse(Path::new(r"C:\x\Blade Runner 2049.mkv"), false);
        assert_eq!((no_year.kind.as_str(), no_year.title.as_str(), no_year.year), ("unsorted", "Blade Runner 2049", None));
    }

    #[test]
    fn anime_names_keep_their_season_and_long_running_numbers() {
        assert_eq!(ep(r"C:\x\[SubsPlease] Mushoku Tensei S2 - 05 (1080p).mkv"), ("Mushoku Tensei".into(), 2, 5, None, "Episode 5".into()));
        assert_eq!(ep(r"C:\x\One Piece - 1071 [1080p].mkv"), ("One Piece".into(), 1, 1071, None, "Episode 1071".into()));
        let concert = parse(Path::new(r"C:\x\Concert - 2019.mkv"), false);
        assert_eq!((concert.kind.as_str(), concert.title.as_str(), concert.year), ("movie", "Concert", Some(2019)), "a year after a dash is not an episode number");
    }

    #[test]
    fn samples_and_trailers_are_listed_but_not_ticked() {
        let sample = parse(Path::new(r"C:\x\Show.S01E01.sample.mkv"), false);
        assert_eq!((sample.kind.as_str(), sample.include), ("episode", false));
        assert!(!parse(Path::new(r"C:\x\Sample\show.s01e01.mkv"), false).include);
        assert!(parse(Path::new(r"C:\x\Trailer.Park.Boys.S01E01.mkv"), false).include, "a title word is not a tag");
    }

    fn ep(path: &str) -> (String, i64, i64, Option<i64>, String) {
        let p = parse(Path::new(path), false);
        assert_eq!(p.kind, "episode", "{path} should be an episode");
        (p.series, p.season, p.episode, p.episode_end, p.title)
    }

    #[test]
    fn reads_scene_style_names() {
        assert_eq!(
            ep(r"C:\V\WandaVision.S01.COMPLETE.HDR.2160p[TGx]\WandaVision.S01E07.Breaking.the.Fourth.Wall.HDR.2160p.WEB-DL.DDP.5.1.x265-TOMMY.mkv"),
            ("WandaVision".into(), 1, 7, None, "Breaking the Fourth Wall".into())
        );
    }

    #[test]
    fn falls_back_to_episode_number_when_there_is_no_title() {
        assert_eq!(
            ep(r"C:\V\VisionQuest.S01E01.2160p.DSNP.WEB-DL.DV.HDR10+[Ben The Men].mp4"),
            ("VisionQuest".into(), 1, 1, None, "Episode 1".into())
        );
    }

    #[test]
    fn reads_dash_separated_names_and_drops_the_year() {
        assert_eq!(
            ep(r"C:\D\Suits (2011) Season 1-9 (1080p BluRay x265)\Season 02\Suits (2011) - S02E01 - She Knows (1080p BluRay x265 ImE).mkv"),
            ("Suits".into(), 2, 1, None, "She Knows".into())
        );
    }

    #[test]
    fn accepts_single_digit_episode_and_lowercase() {
        assert_eq!(ep(r"C:\x\the.bear.s02e1.mkv"), ("The Bear".into(), 2, 1, None, "Episode 1".into()));
    }

    #[test]
    fn reads_double_episodes() {
        assert_eq!(ep(r"C:\x\Show.S01E01E02.720p.mkv"), ("Show".into(), 1, 1, Some(2), "Episode 1".into()));
    }

    #[test]
    fn reads_1x02_style_but_not_resolutions() {
        assert_eq!(ep(r"C:\x\Show - 1x02 - Second.avi"), ("Show".into(), 1, 2, None, "Second".into()));
        assert_eq!(parse(Path::new(r"C:\x\Clip 1920x1080.mp4"), false).kind, "unsorted");
    }

    #[test]
    fn takes_series_and_season_from_folders_for_episode_n_names() {
        assert_eq!(ep(r"C:\x\Dark\Season 3\Episode 4.mkv"), ("Dark".into(), 3, 4, None, "Episode 4".into()));
    }

    #[test]
    fn reads_anime_style_absolute_numbers() {
        assert_eq!(
            ep(r"C:\x\[Group] Frieren - 14 [1080p].mkv"),
            ("Frieren".into(), 1, 14, None, "Episode 14".into())
        );
    }

    #[test]
    fn numbered_lessons_need_siblings() {
        let path = Path::new(r"C:\x\[Site] - IP Networks Course\3. Protocols\1. Address Resolution Protocol (ARP).mp4");
        assert_eq!(parse(path, false).kind, "unsorted");
        let p = parse(path, true);
        assert_eq!((p.series.as_str(), p.season, p.episode), ("IP Networks Course", 3, 1));
        assert_eq!(p.season_title.as_deref(), Some("Protocols"));
        assert_eq!(p.title, "Address Resolution Protocol (ARP)");
    }

    #[test]
    fn files_with_a_year_are_movies_and_others_are_left_out() {
        let movie = parse(Path::new(r"C:\x\Heat (1995) 1080p BluRay.mkv"), false);
        assert_eq!((movie.kind.as_str(), movie.title.as_str(), movie.year, movie.include), ("movie", "Heat", Some(1995), true));
        let stray = parse(Path::new(r"C:\x\Deleted Scenes.mkv"), false);
        assert_eq!((stray.kind.as_str(), stray.include), ("unsorted", false));
    }

    /// (series, season, episode, title, include, after_season)
    fn sp(path: &str) -> (String, i64, i64, String, bool, Option<i64>) {
        let p = parse(Path::new(path), false);
        assert_eq!((p.kind.as_str(), p.season), ("episode", 0), "{path} should be a special");
        (p.series, p.season, p.episode, p.title, p.include, p.after_season)
    }

    #[test]
    fn an_ova_without_a_number_is_a_special_numbered_later() {
        assert_eq!(sp(r"C:\x\[Group] Frieren - OVA [1080p].mkv"), ("Frieren".into(), 0, 0, "OVA".into(), true, None));
    }

    #[test]
    fn sp01_is_special_1() {
        assert_eq!(sp(r"C:\x\[SubsPlease] Bocchi the Rock! - SP01 (1080p) [ABCD1234].mkv"), ("Bocchi the Rock!".into(), 0, 1, "Special 1".into(), true, None));
    }

    #[test]
    fn oav_oad_and_picture_drama_are_specials_too() {
        assert_eq!(sp(r"C:\x\Hellsing Ultimate OAV 03.mkv").3, "OVA 3");
        assert_eq!(sp(r"C:\x\[Group] Attack on Titan - OAD 2 [BD].mkv").3, "OAD 2");
        assert_eq!(sp(r"C:\x\Toradora - Picture Drama 1.mkv").3, "Picture drama 1");
    }

    #[test]
    fn a_number_after_a_dash_belongs_to_the_ova_not_to_an_episode() {
        assert_eq!(sp(r"C:\x\[Group] Show OVA - 02 [720p].mkv"), ("Show".into(), 0, 2, "OVA 2".into(), true, None));
    }

    #[test]
    fn a_season_tag_before_the_special_says_where_it_airs() {
        assert_eq!(sp(r"C:\x\Mushoku Tensei S2 OVA 2 - The Beach.mkv"), ("Mushoku Tensei".into(), 0, 2, "The Beach".into(), true, Some(2)));
    }

    #[test]
    fn a_special_in_a_season_folder_airs_after_that_season() {
        assert_eq!(sp(r"D:\TV\Mushoku Tensei\Season 2\Mushoku Tensei - OVA.mkv").5, Some(2));
    }

    #[test]
    fn a_christmas_special_keeps_its_episode_title() {
        assert_eq!(
            sp(r"D:\TV\Doctor Who (2005) - The Runaway Bride (Christmas Special).mkv"),
            ("Doctor Who".into(), 0, 0, "The Runaway Bride".into(), true, None)
        );
        assert_eq!(sp(r"D:\TV\The Office - Christmas Special 2.mkv").2, 2);
    }

    #[test]
    fn special_as_an_ordinary_word_is_not_a_special() {
        let film = parse(Path::new(r"C:\x\Special Forces (2011).mkv"), false);
        assert_eq!((film.kind.as_str(), film.title.as_str()), ("movie", "Special Forces"));
        assert_eq!(ep(r"C:\x\[Group] Show - 05 - Special Training [1080p].mkv"), ("Show".into(), 1, 5, None, "Episode 5".into()));
        assert_eq!(parse(Path::new(r"C:\x\Casanova (2005).mkv"), false).kind, "movie", "OVA inside a word");
    }

    #[test]
    fn an_explicit_s00e_number_still_wins() {
        assert_eq!(ep(r"C:\x\Dark.S00E02.OVA.mkv"), ("Dark".into(), 0, 2, None, "OVA".into()));
    }

    #[test]
    fn everything_in_a_specials_folder_belongs_to_the_series_above() {
        assert_eq!(sp(r"D:\TV\Dark\Specials\Dark - Making Of.mkv"), ("Dark".into(), 0, 0, "Making Of".into(), true, None));
        assert_eq!(sp(r"D:\TV\Dark\Specials\Episode 3.mkv"), ("Dark".into(), 0, 3, "Episode 3".into(), true, None));
        assert_eq!(sp(r"D:\TV\Dark\Season 00\Behind Dark.mkv").0, "Dark");
    }

    #[test]
    fn extras_folders_are_offered_as_specials_but_not_ticked() {
        assert_eq!(sp(r"D:\TV\Severance\Extras\Behind the Scenes.mkv"), ("Severance".into(), 0, 0, "Behind the Scenes".into(), false, None));
        assert_eq!(sp(r"D:\TV\Severance\Season 1\Featurettes\Cast Interview.mkv"), ("Severance".into(), 0, 0, "Cast Interview".into(), false, Some(1)));
    }

    #[test]
    fn creditless_openings_are_extras() {
        assert_eq!(sp(r"C:\x\[Group] Frieren - NCOP1 [BD 1080p].mkv"), ("Frieren".into(), 0, 1, "Creditless opening 1".into(), false, None));
        assert_eq!(sp(r"C:\x\[Group] Frieren - NCED [BD 1080p].mkv").3, "Creditless ending");
    }

    #[test]
    fn unnumbered_specials_take_the_next_free_numbers_in_their_series() {
        let mut found: Vec<Proposed> = [r"C:\x\Show - SP02.mkv", r"C:\x\Show - OVA.mkv", r"C:\x\Show - OVA - Beach.mkv", r"C:\y\Other - OVA.mkv"]
            .iter()
            .map(|p| parse(Path::new(p), false))
            .collect();
        number_specials(&mut found);
        let numbers: Vec<(&str, i64)> = found.iter().map(|p| (p.series.as_str(), p.episode)).collect();
        assert_eq!(numbers, vec![("Show", 2), ("Show", 3), ("Show", 4), ("Other", 1)]);
    }

    #[test]
    fn match_key_ignores_case_and_punctuation() {
        assert_eq!(match_key("Mr. Robot"), match_key("mr robot"));
        assert_ne!(match_key("Dark"), match_key("Dark Matter"));
    }
}
