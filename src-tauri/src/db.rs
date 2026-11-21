//! SQLite store for the library and watch progress. Video files are only ever
//! referenced by path; nothing here moves, copies or renames them.

use crate::organiser::{match_key, Proposed};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const SCHEMA_V1: &str = "
CREATE TABLE media_files (
  id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size_bytes INTEGER NOT NULL DEFAULT 0,
  duration_ms INTEGER NOT NULL DEFAULT 0, width INTEGER NOT NULL DEFAULT 0, height INTEGER NOT NULL DEFAULT 0,
  thumb_state INTEGER NOT NULL DEFAULT 0  -- 0 pending, 1 ready, 2 failed
);
CREATE TABLE series (
  id INTEGER PRIMARY KEY, title TEXT NOT NULL, match_key TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL
);
CREATE TABLE seasons (
  id INTEGER PRIMARY KEY, series_id INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
  number INTEGER NOT NULL, title TEXT, UNIQUE(series_id, number)
);
CREATE TABLE episodes (
  id INTEGER PRIMARY KEY, season_id INTEGER NOT NULL REFERENCES seasons(id) ON DELETE CASCADE,
  number INTEGER NOT NULL, number_end INTEGER, title TEXT NOT NULL,
  media_file_id INTEGER NOT NULL UNIQUE REFERENCES media_files(id) ON DELETE CASCADE
);
CREATE TABLE movies (
  id INTEGER PRIMARY KEY, title TEXT NOT NULL, year INTEGER,
  media_file_id INTEGER NOT NULL UNIQUE REFERENCES media_files(id) ON DELETE CASCADE
);
CREATE TABLE playback (
  media_file_id INTEGER PRIMARY KEY REFERENCES media_files(id) ON DELETE CASCADE,
  position_ms INTEGER NOT NULL DEFAULT 0, watched INTEGER NOT NULL DEFAULT 0, last_played_at INTEGER
);
";

/// Added for per-series track memory, watch folders, and so that a watch folder
/// does not bring back something the user removed.
const SCHEMA_V2: &str = "
CREATE TABLE series_prefs (
  series_id INTEGER PRIMARY KEY REFERENCES series(id) ON DELETE CASCADE,
  audio_lang TEXT, sub_lang TEXT  -- sub_lang 'no' means subtitles off
);
CREATE TABLE watch_folders (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE);
CREATE TABLE removed_paths (path TEXT PRIMARY KEY);
";

/// Where a special plays in watch order, TheTVDB's "airs after": after the last episode of
/// `after_season` numbered `after_number` or less (0 = before the season starts, NULL = after
/// its end). NULL `after_season` leaves it at the end, after every numbered season.
const SCHEMA_V3: &str = "
ALTER TABLE episodes ADD COLUMN after_season INTEGER;
ALTER TABLE episodes ADD COLUMN after_number INTEGER;
";

/// One episode as watch order sees it.
pub struct OrderEntry {
    pub season: i64,
    pub number: i64,
    /// For a special: (season, episode number it follows; 0 before the season, None after it).
    pub after: Option<(i64, Option<i64>)>,
}

/// The order a series is watched in, as indexes into `entries`, which come in display order
/// (numbered seasons, then specials; each by number). A placed special goes right after the
/// episode it follows; one placed in a season that isn't there, or not placed, goes at the end.
pub fn watch_order(entries: &[OrderEntry]) -> Vec<usize> {
    let numbered: Vec<usize> = (0..entries.len()).filter(|&i| entries[i].season != 0).collect();
    // Slot k means "just before numbered[k]"; numbered.len() is after everything. The flag
    // marks a special that opens a season, so it follows one that closes the season before.
    let slot = |season: i64, number: Option<i64>| -> Option<(usize, bool)> {
        let in_season: Vec<usize> = (0..numbered.len()).filter(|&k| entries[numbered[k]].season == season).collect();
        let (first, last) = (*in_season.first()?, *in_season.last()?);
        Some(match number {
            None => (last + 1, false),
            Some(n) => in_season.iter().rev().find(|&&k| entries[numbered[k]].number <= n).map_or((first, true), |&k| (k + 1, false)),
        })
    };
    let mut placed: Vec<Vec<(bool, usize)>> = vec![Vec::new(); numbered.len() + 1];
    let mut unplaced = Vec::new();
    for i in (0..entries.len()).filter(|&i| entries[i].season == 0) {
        match entries[i].after.and_then(|(s, n)| slot(s, n)) {
            Some((k, opens)) => placed[k].push((opens, i)),
            None => unplaced.push(i),
        }
    }
    for specials in &mut placed {
        specials.sort_by_key(|(opens, _)| *opens);
    }
    let placed: Vec<Vec<usize>> = placed.into_iter().map(|s| s.into_iter().map(|(_, i)| i).collect()).collect();
    let mut order = Vec::with_capacity(entries.len());
    for (k, specials) in placed.into_iter().enumerate() {
        order.extend(specials);
        order.extend(numbered.get(k));
    }
    order.extend(unplaced);
    order
}

/// An item counts as watched at 92%, or when under two minutes remain (the credits).
/// The two-minute rule is for full-length episodes only: without the length check a
/// ninety-second clip would be "watched" the moment it starts.
pub fn is_watched(position_ms: i64, duration_ms: i64) -> bool {
    if duration_ms <= 0 {
        return false;
    }
    let in_credits = duration_ms >= 10 * 60_000 && duration_ms - position_ms < 120_000;
    position_ms * 100 >= duration_ms * 92 || in_credits
}

/// Where playback should start: a little before the saved point, or the beginning
/// when the last save was in the credits or the first seconds. It goes by position
/// alone, so a finished episode opened again to find a scene resumes there too.
pub fn resume_position_ms(position_ms: i64, duration_ms: i64) -> i64 {
    if position_ms < 10_000 || is_watched(position_ms, duration_ms) { 0 } else { position_ms - 3_000 }
}

pub struct EpisodeProgress {
    pub watched: bool,
    pub position_ms: i64,
    pub last_played_at: Option<i64>,
}

/// Which episode the series Play button starts, given episodes in watch order:
/// the most recently played unfinished one (unless something was finished after
/// it, which means it was abandoned), else the first unwatched after the last
/// watched, else the first unwatched anywhere (specials sort last, so a watched
/// special must not hide what is left), else the first.
pub fn pick_resume(episodes: &[EpisodeProgress]) -> Option<usize> {
    if episodes.is_empty() {
        return None;
    }
    let last_finished = episodes.iter().filter(|e| e.watched).filter_map(|e| e.last_played_at).max().unwrap_or(0);
    let in_progress = episodes
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.watched && e.position_ms > 0 && e.last_played_at.unwrap_or(0) > last_finished)
        .max_by_key(|(_, e)| e.last_played_at.unwrap_or(0));
    if let Some((i, _)) = in_progress {
        return Some(i);
    }
    let after_last_watched = episodes.iter().rposition(|e| e.watched).map_or(0, |i| i + 1);
    Some(
        episodes[after_last_watched..]
            .iter()
            .position(|e| !e.watched)
            .map(|i| i + after_last_watched)
            .or_else(|| episodes.iter().position(|e| !e.watched))
            .unwrap_or(0),
    )
}

/// Pairs library files that are no longer at their path with files found elsewhere:
/// same name and size first, then same size when only one file on each side has it.
/// `missing` is (media file id, old path, size); `found` is (path, size).
pub fn match_moved(missing: &[(i64, String, i64)], found: &[(String, i64)]) -> Vec<(i64, String)> {
    let name = |p: &str| p.rsplit(['\\', '/']).next().unwrap_or_default().to_lowercase();
    let mut matches: Vec<(i64, String)> = Vec::new();
    let mut used: Vec<&str> = Vec::new();
    for (id, old, size) in missing {
        if let Some((path, _)) = found.iter().find(|(p, s)| s == size && name(p) == name(old) && !used.contains(&p.as_str())) {
            used.push(path);
            matches.push((*id, path.clone()));
        }
    }
    for (id, _, size) in missing {
        if *size <= 0 || matches.iter().any(|(m, _)| m == id) {
            continue;
        }
        let same: Vec<&(String, i64)> = found.iter().filter(|(p, s)| s == size && !used.contains(&p.as_str())).collect();
        let rivals = missing.iter().filter(|(other, _, s)| s == size && !matches.iter().any(|(m, _)| m == other)).count();
        if let ([only], 1) = (same.as_slice(), rivals) {
            used.push(&only.0);
            matches.push((*id, only.0.clone()));
        }
    }
    matches
}

#[derive(Serialize)]
pub struct WatchFolder {
    pub id: i64,
    pub path: String,
}

#[derive(Serialize, Clone)]
pub struct Episode {
    pub id: i64,
    pub season_number: i64,
    pub number: i64,
    pub number_end: Option<i64>,
    pub title: String,
    pub media_file_id: i64,
    pub path: String,
    pub has_thumb: bool,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub watched: bool,
    pub last_played_at: Option<i64>,
    /// For a special: where it plays (see SCHEMA_V3).
    pub after_season: Option<i64>,
    pub after_number: Option<i64>,
}

#[derive(Serialize)]
pub struct Season {
    pub id: i64,
    pub number: i64,
    pub title: Option<String>,
    pub episodes: Vec<Episode>,
}

#[derive(Serialize)]
pub struct Series {
    pub id: i64,
    pub title: String,
    pub seasons: Vec<Season>,
    /// Episode the Play button starts.
    pub resume_episode_id: Option<i64>,
    /// Every episode id in the order it is watched, with placed specials in their spots.
    pub watch_order: Vec<i64>,
}

#[derive(Serialize)]
pub struct Movie {
    pub id: i64,
    pub title: String,
    pub year: Option<i64>,
    pub media_file_id: i64,
    pub path: String,
    pub has_thumb: bool,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub watched: bool,
    pub last_played_at: Option<i64>,
}

#[derive(Serialize)]
pub struct Library {
    pub series: Vec<Series>,
    pub movies: Vec<Movie>,
    pub thumbs_dir: String,
}

pub struct Db {
    conn: Mutex<Connection>,
    thumbs_dir: PathBuf,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

type Res<T> = Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Db {
    pub fn open(data_dir: &Path) -> Res<Self> {
        fs::create_dir_all(data_dir).map_err(err)?;
        let thumbs_dir = data_dir.join("thumbs");
        fs::create_dir_all(&thumbs_dir).map_err(err)?;
        Self::with_connection(Connection::open(data_dir.join("library.db")).map_err(err)?, thumbs_dir)
    }

    fn with_connection(conn: Connection, thumbs_dir: PathBuf) -> Res<Self> {
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        conn.pragma_update(None, "foreign_keys", "ON").map_err(err)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
        if version < 1 {
            // One transaction, so a crash can't leave half the tables behind.
            conn.execute_batch(&format!("BEGIN;{SCHEMA_V1}PRAGMA user_version = 1;COMMIT;")).map_err(err)?;
        }
        if version < 2 {
            conn.execute_batch(&format!("BEGIN;{SCHEMA_V2}PRAGMA user_version = 2;COMMIT;")).map_err(err)?;
        }
        if version < 3 {
            conn.execute_batch(&format!("BEGIN;{SCHEMA_V3}PRAGMA user_version = 3;COMMIT;")).map_err(err)?;
        }
        // Files whose thumbnail failed last time (drive unplugged, say) get another go each launch.
        conn.execute("UPDATE media_files SET thumb_state = 0 WHERE thumb_state = 2", []).map_err(err)?;
        Ok(Self { conn: Mutex::new(conn), thumbs_dir })
    }

    pub fn thumb_path(&self, media_file_id: i64) -> PathBuf {
        self.thumbs_dir.join(format!("{media_file_id}.jpg"))
    }

    /// Cached scrubber frame for one ten-second step of a file.
    pub fn preview_path(&self, media_file_id: i64, step: i64) -> PathBuf {
        self.thumbs_dir.join(format!("p{media_file_id}_{step}.jpg"))
    }

    fn media_file(conn: &Connection, path: &str, size: i64) -> Res<Option<i64>> {
        // A path already in the library is skipped, so re-importing a folder is harmless.
        let exists: Option<i64> = conn
            .query_row("SELECT id FROM media_files WHERE path = ?1", [path], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if exists.is_some() {
            return Ok(None);
        }
        conn.execute("INSERT INTO media_files (path, size_bytes) VALUES (?1, ?2)", params![path, size]).map_err(err)?;
        Ok(Some(conn.last_insert_rowid()))
    }

    fn series_id(conn: &Connection, title: &str) -> Res<i64> {
        let key = match_key(title);
        if let Some(id) = conn
            .query_row("SELECT id FROM series WHERE match_key = ?1", [&key], |r| r.get(0))
            .optional()
            .map_err(err)?
        {
            return Ok(id);
        }
        conn.execute("INSERT INTO series (title, match_key, created_at) VALUES (?1, ?2, ?3)", params![title, key, now()]).map_err(err)?;
        Ok(conn.last_insert_rowid())
    }

    fn season_id(conn: &Connection, series_id: i64, number: i64, title: Option<&str>) -> Res<i64> {
        if let Some(id) = conn
            .query_row("SELECT id FROM seasons WHERE series_id = ?1 AND number = ?2", params![series_id, number], |r| r.get(0))
            .optional()
            .map_err(err)?
        {
            return Ok(id);
        }
        conn.execute("INSERT INTO seasons (series_id, number, title) VALUES (?1, ?2, ?3)", params![series_id, number, title]).map_err(err)?;
        Ok(conn.last_insert_rowid())
    }

    /// Adds the reviewed import to the library, merging into series that already exist.
    /// Returns how many files were new.
    pub fn import(&self, items: &[Proposed]) -> Res<usize> {
        self.import_from(items, false)
    }

    /// Import from a watch folder: the same, except that anything the user removed
    /// from the library stays out.
    pub fn import_watched(&self, items: &[Proposed]) -> Res<usize> {
        self.import_from(items, true)
    }

    fn import_from(&self, items: &[Proposed], automatic: bool) -> Res<usize> {
        // File sizes are read before the lock is taken: a slow drive must not hold up playback saves.
        let chosen: Vec<(&Proposed, i64)> = items
            .iter()
            .filter(|i| i.include)
            .map(|i| (i, fs::metadata(&i.path).map(|m| m.len() as i64).unwrap_or(0)))
            .collect();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(err)?;
        let mut added = 0;
        for (item, size) in chosen {
            let removed: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM removed_paths WHERE path = ?1)", [&item.path], |r| r.get(0))
                .map_err(err)?;
            if removed && automatic {
                continue;
            }
            // Adding it by hand again overrules the earlier removal.
            tx.execute("DELETE FROM removed_paths WHERE path = ?1", [&item.path]).map_err(err)?;
            let Some(media_id) = Self::media_file(&tx, &item.path, size)? else { continue };
            if item.kind == "episode" && !item.series.trim().is_empty() {
                let series_id = Self::series_id(&tx, item.series.trim())?;
                let season_id = Self::season_id(&tx, series_id, item.season, item.season_title.as_deref())?;
                let (number, after_season) = if item.season == 0 {
                    // Specials come from many sources ("OVA 1", "SP1"), so one that would share a
                    // number with a special already there takes the next free one.
                    (Self::free_special_number(&tx, season_id, item.episode)?, item.after_season)
                } else {
                    (item.episode, None)
                };
                tx.execute(
                    "INSERT INTO episodes (season_id, number, number_end, title, media_file_id, after_season) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![season_id, number, item.episode_end, item.title, media_id, after_season],
                )
                .map_err(err)?;
            } else {
                tx.execute("INSERT INTO movies (title, year, media_file_id) VALUES (?1, ?2, ?3)", params![item.title, item.year, media_id]).map_err(err)?;
            }
            added += 1;
        }
        tx.commit().map_err(err)?;
        Ok(added)
    }

    /// `wanted` if no episode in the season has it (and it is above 0), else the next number up.
    fn free_special_number(conn: &Connection, season_id: i64, wanted: i64) -> Res<i64> {
        let taken: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM episodes WHERE season_id = ?1 AND number = ?2)", params![season_id, wanted], |r| r.get(0))
            .map_err(err)?;
        if wanted > 0 && !taken {
            return Ok(wanted);
        }
        conn.query_row("SELECT COALESCE(MAX(number), 0) + 1 FROM episodes WHERE season_id = ?1", [season_id], |r| r.get(0)).map_err(err)
    }

    pub fn library(&self) -> Res<Library> {
        let conn = self.conn.lock().unwrap();
        let mut episodes: HashMap<i64, Vec<Episode>> = HashMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.season_id, s.number, e.number, e.number_end, e.title, m.id, m.path, m.thumb_state,
                        m.duration_ms, COALESCE(p.position_ms, 0), COALESCE(p.watched, 0), p.last_played_at,
                        e.after_season, e.after_number
                 FROM episodes e JOIN seasons s ON s.id = e.season_id JOIN media_files m ON m.id = e.media_file_id
                 LEFT JOIN playback p ON p.media_file_id = m.id ORDER BY e.number, e.id",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(1)?,
                    Episode {
                        id: r.get(0)?,
                        season_number: r.get(2)?,
                        number: r.get(3)?,
                        number_end: r.get(4)?,
                        title: r.get(5)?,
                        media_file_id: r.get(6)?,
                        path: r.get(7)?,
                        has_thumb: r.get::<_, i64>(8)? == 1,
                        duration_ms: r.get(9)?,
                        position_ms: r.get(10)?,
                        watched: r.get::<_, i64>(11)? == 1,
                        last_played_at: r.get(12)?,
                        after_season: r.get(13)?,
                        after_number: r.get(14)?,
                    },
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (season_id, episode) = row.map_err(err)?;
            episodes.entry(season_id).or_default().push(episode);
        }

        let mut seasons: HashMap<i64, Vec<Season>> = HashMap::new();
        let mut stmt = conn.prepare("SELECT id, series_id, number, title FROM seasons ORDER BY number = 0, number").map_err(err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(1)?, r.get::<_, i64>(0)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?)))
            .map_err(err)?;
        for row in rows {
            let (series_id, id, number, title) = row.map_err(err)?;
            seasons.entry(series_id).or_default().push(Season { id, number, title, episodes: episodes.remove(&id).unwrap_or_default() });
        }

        let mut stmt = conn.prepare("SELECT id, title FROM series ORDER BY created_at, id").map_err(err)?;
        let series = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(err)?
            .map(|row| {
                let (id, title) = row.map_err(err)?;
                let seasons = seasons.remove(&id).unwrap_or_default();
                let shown: Vec<&Episode> = seasons.iter().flat_map(|s| &s.episodes).collect();
                let entries: Vec<OrderEntry> = shown
                    .iter()
                    .map(|e| OrderEntry { season: e.season_number, number: e.number, after: e.after_season.map(|s| (s, e.after_number)) })
                    .collect();
                let ordered: Vec<&Episode> = watch_order(&entries).into_iter().map(|i| shown[i]).collect();
                let progress: Vec<EpisodeProgress> = ordered
                    .iter()
                    .map(|e| EpisodeProgress { watched: e.watched, position_ms: e.position_ms, last_played_at: e.last_played_at })
                    .collect();
                let resume_episode_id = pick_resume(&progress).map(|i| ordered[i].id);
                let watch_order = ordered.iter().map(|e| e.id).collect();
                Ok(Series { id, title, seasons, resume_episode_id, watch_order })
            })
            .collect::<Res<Vec<_>>>()?;

        let mut stmt = conn
            .prepare(
                "SELECT v.id, v.title, v.year, m.id, m.path, m.thumb_state, m.duration_ms,
                        COALESCE(p.position_ms, 0), COALESCE(p.watched, 0), p.last_played_at
                 FROM movies v JOIN media_files m ON m.id = v.media_file_id
                 LEFT JOIN playback p ON p.media_file_id = m.id ORDER BY v.title",
            )
            .map_err(err)?;
        let movies = stmt
            .query_map([], |r| {
                Ok(Movie {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    year: r.get(2)?,
                    media_file_id: r.get(3)?,
                    path: r.get(4)?,
                    has_thumb: r.get::<_, i64>(5)? == 1,
                    duration_ms: r.get(6)?,
                    position_ms: r.get(7)?,
                    watched: r.get::<_, i64>(8)? == 1,
                    last_played_at: r.get(9)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;

        Ok(Library { series, movies, thumbs_dir: self.thumbs_dir.to_string_lossy().into_owned() })
    }

    /// (path, saved position, watched, duration) for a media file.
    pub fn media(&self, media_file_id: i64) -> Res<(String, i64, bool, i64)> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT m.path, COALESCE(p.position_ms, 0), COALESCE(p.watched, 0), m.duration_ms
                 FROM media_files m LEFT JOIN playback p ON p.media_file_id = m.id WHERE m.id = ?1",
                [media_file_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? == 1, r.get(3)?)),
            )
            .map_err(err)
    }

    pub fn save_progress(&self, media_file_id: i64, position_ms: i64, duration_ms: i64) -> Res<()> {
        let conn = self.conn.lock().unwrap();
        if duration_ms > 0 {
            conn.execute("UPDATE media_files SET duration_ms = ?2 WHERE id = ?1", params![media_file_id, duration_ms]).map_err(err)?;
        }
        // Watched is sticky: looking at a finished episode again does not un-watch it.
        // Only the user's own "Mark as unwatched" clears it.
        conn.execute(
            "INSERT INTO playback (media_file_id, position_ms, watched, last_played_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(media_file_id) DO UPDATE SET position_ms = ?2, watched = MAX(watched, ?3), last_played_at = ?4",
            params![media_file_id, position_ms, is_watched(position_ms, duration_ms) as i64, now()],
        )
        .map_err(err)?;
        Ok(())
    }

    /// Marking by hand counts as finishing it now, so an older half-played episode
    /// is not offered ahead of what comes after the marked one.
    pub fn set_watched(&self, media_file_id: i64, watched: bool) -> Res<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO playback (media_file_id, position_ms, watched, last_played_at) VALUES (?1, 0, ?2, CASE WHEN ?2 = 1 THEN ?3 END)
                 ON CONFLICT(media_file_id) DO UPDATE SET watched = ?2, position_ms = 0,
                   last_played_at = CASE WHEN ?2 = 1 THEN ?3 ELSE last_played_at END",
                params![media_file_id, watched as i64, now()],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn rename(&self, table: &str, id: i64, title: &str) -> Res<()> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A name can't be empty. Type a name, or press Escape to keep the old one.".into());
        }
        if table == "series" {
            // The merge key follows the name, so files added later still land in this series.
            return self
                .conn
                .lock()
                .unwrap()
                .execute("UPDATE series SET title = ?2, match_key = ?3 WHERE id = ?1", params![id, title, match_key(title)])
                .map(|_| ())
                .map_err(|_| format!("There is already a series called {title}. Pick a different name."));
        }
        let sql = match table {
            "season" => "UPDATE seasons SET title = ?2 WHERE id = ?1",
            "episode" => "UPDATE episodes SET title = ?2 WHERE id = ?1",
            "movie" => "UPDATE movies SET title = ?2 WHERE id = ?1",
            _ => return Err(format!("Unknown item type: {table}")),
        };
        self.conn.lock().unwrap().execute(sql, params![id, title]).map(|_| ()).map_err(err)
    }

    pub fn create_series(&self, title: &str) -> Res<i64> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Give the series a name first.".into());
        }
        let conn = self.conn.lock().unwrap();
        let id = Self::series_id(&conn, title)?;
        Self::season_id(&conn, id, 1, None)?;
        Ok(id)
    }

    pub fn add_season(&self, series_id: i64) -> Res<i64> {
        let conn = self.conn.lock().unwrap();
        let next: i64 = conn
            .query_row("SELECT COALESCE(MAX(number), 0) + 1 FROM seasons WHERE series_id = ?1", [series_id], |r| r.get(0))
            .map_err(err)?;
        Self::season_id(&conn, series_id, next, None)?;
        Ok(next)
    }

    /// Moves an episode to another season of the same series, creating it if needed,
    /// and optionally renumbers it. An episode moved into Specials keeps its spot in watch
    /// order and takes the next free special number; one moved out loses its placement.
    pub fn move_episode(&self, episode_id: i64, season_number: i64, number: Option<i64>) -> Res<()> {
        if season_number < 0 || number.is_some_and(|n| n < 0) {
            return Err("Season and episode numbers can't be negative. Use 0 for Specials.".into());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(err)?;
        let (series_id, from_season, from_number, after_season, after_number): (i64, i64, i64, Option<i64>, Option<i64>) = tx
            .query_row(
                "SELECT s.series_id, s.number, e.number, e.after_season, e.after_number FROM episodes e JOIN seasons s ON s.id = e.season_id WHERE e.id = ?1",
                [episode_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(err)?;
        let season_id = Self::season_id(&tx, series_id, season_number, None)?;
        let (number, after_season, after_number) = match (from_season, season_number) {
            (0, 0) => (number.unwrap_or(from_number), after_season, after_number),
            (_, 0) => (number.map_or_else(|| Self::free_special_number(&tx, season_id, 0), Ok)?, Some(from_season), Some(from_number)),
            _ => (number.unwrap_or(from_number), None, None),
        };
        tx.execute(
            "UPDATE episodes SET season_id = ?2, number = ?3, after_season = ?4, after_number = ?5 WHERE id = ?1",
            params![episode_id, season_id, number, after_season, after_number],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    /// Sets where a special plays: after episode `after_number` of `after_season` (0 = before
    /// the season, None = after its last episode), or at the end when `after_season` is None.
    pub fn place_special(&self, episode_id: i64, after_season: Option<i64>, after_number: Option<i64>) -> Res<()> {
        let conn = self.conn.lock().unwrap();
        let season: i64 = conn
            .query_row("SELECT s.number FROM episodes e JOIN seasons s ON s.id = e.season_id WHERE e.id = ?1", [episode_id], |r| r.get(0))
            .map_err(err)?;
        if season != 0 {
            return Err("Only specials can be placed between episodes. Move it to Specials first.".into());
        }
        if after_season == Some(0) {
            return Err("A special can't be placed among the specials. Pick a numbered season.".into());
        }
        let after_number = after_season.and(after_number);
        conn.execute("UPDATE episodes SET after_season = ?2, after_number = ?3 WHERE id = ?1", params![episode_id, after_season, after_number])
            .map(|_| ())
            .map_err(err)
    }


    /// Removes an item from the library. The video files on disk are left alone.
    pub fn remove(&self, table: &str, id: i64) -> Res<()> {
        let mut conn = self.conn.lock().unwrap();
        let sql = match table {
            "series" => "SELECT e.media_file_id FROM episodes e JOIN seasons s ON s.id = e.season_id WHERE s.series_id = ?1",
            "episode" => "SELECT media_file_id FROM episodes WHERE id = ?1",
            "movie" => "SELECT media_file_id FROM movies WHERE id = ?1",
            _ => return Err(format!("Unknown item type: {table}")),
        };
        let media_ids = conn
            .prepare(sql)
            .map_err(err)?
            .query_map([id], |r| r.get::<_, i64>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        // All or nothing, in one commit.
        let tx = conn.transaction().map_err(err)?;
        for media_id in &media_ids {
            tx.execute("INSERT OR IGNORE INTO removed_paths SELECT path FROM media_files WHERE id = ?1", [media_id]).map_err(err)?;
            tx.execute("DELETE FROM media_files WHERE id = ?1", [media_id]).map_err(err)?;
        }
        if table == "series" {
            tx.execute("DELETE FROM series WHERE id = ?1", [id]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        drop(conn);
        // Cached thumbnails and scrubber frames go once the rows are gone, without holding the lock.
        let previews: Vec<String> = media_ids.iter().map(|id| format!("p{id}_")).collect();
        for entry in fs::read_dir(&self.thumbs_dir).into_iter().flatten().flatten() {
            if previews.iter().any(|prefix| entry.file_name().to_string_lossy().starts_with(prefix.as_str())) {
                let _ = fs::remove_file(entry.path());
            }
        }
        for media_id in media_ids {
            let _ = fs::remove_file(self.thumb_path(media_id));
        }
        Ok(())
    }

    /// Remembered (audio language, subtitle language) for the series a file belongs to.
    /// A subtitle language of "no" means subtitles off. Films have no memory.
    pub fn track_prefs(&self, media_file_id: i64) -> (Option<String>, Option<String>) {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT p.audio_lang, p.sub_lang FROM episodes e JOIN seasons s ON s.id = e.season_id
                 JOIN series_prefs p ON p.series_id = s.series_id WHERE e.media_file_id = ?1",
                [media_file_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((None, None))
    }

    /// Remembers a track choice for the whole series. `kind` is "audio" or "sub".
    pub fn set_track_pref(&self, media_file_id: i64, kind: &str, lang: &str) -> Res<()> {
        let column = match kind {
            "audio" => "audio_lang",
            "sub" => "sub_lang",
            _ => return Err(format!("Unknown track type: {kind}")),
        };
        self.conn
            .lock()
            .unwrap()
            .execute(
                &format!(
                    "INSERT INTO series_prefs (series_id, {column})
                     SELECT s.series_id, ?2 FROM episodes e JOIN seasons s ON s.id = e.season_id WHERE e.media_file_id = ?1
                     ON CONFLICT(series_id) DO UPDATE SET {column} = ?2"
                ),
                params![media_file_id, lang],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn watch_folders(&self) -> Res<Vec<WatchFolder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, path FROM watch_folders ORDER BY path").map_err(err)?;
        let rows = stmt.query_map([], |r| Ok(WatchFolder { id: r.get(0)?, path: r.get(1)? })).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }

    pub fn add_watch_folder(&self, path: &str) -> Res<()> {
        self.conn.lock().unwrap().execute("INSERT OR IGNORE INTO watch_folders (path) VALUES (?1)", [path]).map(|_| ()).map_err(err)
    }

    pub fn remove_watch_folder(&self, id: i64) -> Res<()> {
        self.conn.lock().unwrap().execute("DELETE FROM watch_folders WHERE id = ?1", [id]).map(|_| ()).map_err(err)
    }

    /// (media file id, path, size) of every file in the library.
    pub fn files(&self) -> Res<Vec<(i64, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, path, size_bytes FROM media_files").map_err(err)?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }

    /// Points library entries at where their files are now. Names, placement and
    /// watch progress stay as they were. Returns how many were updated.
    pub fn relink(&self, moves: &[(i64, String)]) -> Res<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(err)?;
        let mut done = 0;
        for (id, path) in moves {
            let taken: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM media_files WHERE path = ?1 AND id != ?2)", params![path, id], |r| r.get(0))
                .map_err(err)?;
            if taken {
                if moves.len() == 1 {
                    return Err("That file is already in your library as another item. Pick a different file, or remove the other item first.".into());
                }
                continue;
            }
            tx.execute("DELETE FROM removed_paths WHERE path = ?1", [path]).map_err(err)?;
            // The thumbnail is made again in case this is a different copy of the video.
            done += tx.execute("UPDATE media_files SET path = ?2, thumb_state = 0 WHERE id = ?1", params![id, path]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(done)
    }

    /// Next file after id that still needs a thumbnail and its details read.
    pub fn next_unprobed(&self, after: i64) -> Option<(i64, String)> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT id, path FROM media_files WHERE thumb_state = 0 AND id > ?1 ORDER BY id LIMIT 1", [after], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_probed(&self, media_file_id: i64, ok: bool, duration_ms: i64, width: i64, height: i64) {
        let _ = self.conn.lock().unwrap().execute(
            "UPDATE media_files SET thumb_state = ?2, duration_ms = MAX(duration_ms, ?3), width = ?4, height = ?5 WHERE id = ?1",
            params![media_file_id, if ok { 1 } else { 2 }, duration_ms, width, height],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(watched: bool, position_ms: i64, last_played_at: Option<i64>) -> EpisodeProgress {
        EpisodeProgress { watched, position_ms, last_played_at }
    }

    fn proposed(path: &str, series: &str, season: i64, episode: i64) -> Proposed {
        Proposed {
            path: path.into(), kind: "episode".into(), series: series.into(), season, season_title: None,
            episode, episode_end: None, title: format!("Episode {episode}"), year: None, include: true, after_season: None,
        }
    }

    fn memory_db() -> Db {
        Db::with_connection(Connection::open_in_memory().unwrap(), std::env::temp_dir().join("marquee-test-thumbs")).unwrap()
    }

    #[test]
    fn moved_files_are_matched_by_name_and_size_then_by_size_alone() {
        let missing = [(1, r"D:\Old\a.mkv".to_string(), 100), (2, r"D:\Old\b.mkv".to_string(), 200), (3, r"D:\Old\c.mkv".to_string(), 300), (4, r"D:\Old\d.mkv".to_string(), 300)];
        let found = [(r"E:\New\A.MKV".to_string(), 100), (r"E:\New\renamed.mkv".to_string(), 200), (r"E:\New\x.mkv".to_string(), 300), (r"E:\New\other.mkv".to_string(), 999)];
        let matched = match_moved(&missing, &found);
        assert_eq!(matched, vec![(1, r"E:\New\A.MKV".to_string()), (2, r"E:\New\renamed.mkv".to_string())], "two missing files share size 300, so that one is left alone");
    }

    #[test]
    fn relinking_keeps_progress_and_refuses_a_file_that_is_already_another_item() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2)]).unwrap();
        let eps = db.library().unwrap().series[0].seasons[0].episodes.clone();
        db.save_progress(eps[0].media_file_id, 600_000, 3_600_000).unwrap();
        assert_eq!(db.relink(&[(eps[0].media_file_id, "b/1.mkv".into())]).unwrap(), 1);
        let after = db.library().unwrap().series[0].seasons[0].episodes[0].clone();
        assert_eq!((after.path.as_str(), after.position_ms, after.title.as_str()), ("b/1.mkv", 600_000, "Episode 1"));
        assert!(db.relink(&[(eps[0].media_file_id, "a/2.mkv".into())]).is_err());
    }

    #[test]
    fn a_watch_folder_does_not_bring_back_what_was_removed_but_adding_by_hand_does() {
        let db = memory_db();
        let files = [proposed("w/1.mkv", "Dark", 1, 1), proposed("w/2.mkv", "Dark", 1, 2)];
        assert_eq!(db.import_watched(&files).unwrap(), 2);
        let second = db.library().unwrap().series[0].seasons[0].episodes[1].id;
        db.remove("episode", second).unwrap();
        assert_eq!(db.import_watched(&files).unwrap(), 0, "the removed episode stays out, the other is already known");
        assert_eq!(db.import(&files).unwrap(), 1, "adding it by hand brings it back");
        assert_eq!(db.import_watched(&files).unwrap(), 0);
        assert_eq!(db.library().unwrap().series[0].seasons[0].episodes.len(), 2);
    }

    #[test]
    fn track_choices_are_remembered_per_series_and_not_for_films() {
        let db = memory_db();
        let mut film = proposed("m/heat.mkv", "", 0, 0);
        film.kind = "movie".into();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2), proposed("b/1.mkv", "Suits", 1, 1), film]).unwrap();
        let lib = db.library().unwrap();
        let dark = &lib.series.iter().find(|s| s.title == "Dark").unwrap().seasons[0].episodes;
        let suits = &lib.series.iter().find(|s| s.title == "Suits").unwrap().seasons[0].episodes;
        db.set_track_pref(dark[0].media_file_id, "audio", "deu").unwrap();
        db.set_track_pref(dark[0].media_file_id, "sub", "no").unwrap();
        db.set_track_pref(dark[1].media_file_id, "sub", "eng").unwrap();
        assert_eq!(db.track_prefs(dark[1].media_file_id), (Some("deu".into()), Some("eng".into())));
        assert_eq!(db.track_prefs(suits[0].media_file_id), (None, None));
        db.set_track_pref(lib.movies[0].media_file_id, "audio", "fra").unwrap();
        assert_eq!(db.track_prefs(lib.movies[0].media_file_id), (None, None));
    }

    #[test]
    fn a_library_from_the_first_version_gains_the_new_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!("{SCHEMA_V1}PRAGMA user_version = 1;")).unwrap();
        conn.execute("INSERT INTO series (title, match_key, created_at) VALUES ('Dark', 'dark', 1)", []).unwrap();
        let db = Db::with_connection(conn, std::env::temp_dir().join("marquee-test-thumbs")).unwrap();
        assert_eq!(db.library().unwrap().series[0].title, "Dark");
        db.add_watch_folder(r"D:\Shows").unwrap();
        db.add_watch_folder(r"D:\Shows").unwrap();
        assert_eq!(db.watch_folders().unwrap().len(), 1);
    }

    #[test]
    fn watched_at_92_percent_or_under_two_minutes_left() {
        let hour = 3_600_000;
        assert!(!is_watched(hour * 91 / 100, hour));
        assert!(is_watched(hour * 92 / 100, hour));
        assert!(is_watched(9 * 60_000, 10 * 60_000 + 30_000), "1.5 minutes left of a short video");
        assert!(!is_watched(5 * 60_000, 0), "unknown duration is never watched");
    }

    #[test]
    fn a_short_clip_is_not_watched_just_because_it_is_short() {
        assert!(!is_watched(0, 90_000), "a 90 second clip at the start");
        assert!(!is_watched(10_000, 130_000), "ten seconds into a two minute clip");
        assert!(is_watched(85_000, 90_000), "but it is at 92 percent");
    }

    #[test]
    fn looking_at_a_finished_episode_again_keeps_it_watched_and_leaves_play_alone() {
        let hour = 3_600_000;
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2)]).unwrap();
        let eps = db.library().unwrap().series[0].seasons[0].episodes.clone();
        db.save_progress(eps[0].media_file_id, hour - 60_000, hour).unwrap();
        db.save_progress(eps[0].media_file_id, 1_200_000, hour).unwrap();
        let (_, position, watched, duration) = db.media(eps[0].media_file_id).unwrap();
        assert_eq!((watched, resume_position_ms(position, duration)), (true, 1_197_000), "still watched, and opens where the second look stopped");
        assert_eq!(db.library().unwrap().series[0].resume_episode_id, Some(eps[1].id), "Play still goes to the next episode");
    }

    #[test]
    fn a_watched_special_does_not_send_play_back_to_the_start() {
        // Watch order puts specials last: S1 E1 watched, E2 and E3 not, then a watched special.
        let eps = [ep(true, 0, Some(1)), ep(false, 0, None), ep(false, 0, None), ep(true, 0, Some(2))];
        assert_eq!(pick_resume(&eps), Some(1));
    }

    #[test]
    fn marking_later_episodes_watched_by_hand_moves_play_past_a_half_played_one() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2), proposed("a/3.mkv", "Dark", 1, 3)]).unwrap();
        let eps = db.library().unwrap().series[0].seasons[0].episodes.clone();
        db.save_progress(eps[0].media_file_id, 600_000, 3_600_000).unwrap();
        assert_eq!(db.library().unwrap().series[0].resume_episode_id, Some(eps[0].id));
        // Stand in for time passing: the half-played episode was last touched long ago.
        db.conn.lock().unwrap().execute("UPDATE playback SET last_played_at = 1", []).unwrap();
        db.set_watched(eps[1].media_file_id, true).unwrap();
        assert_eq!(db.library().unwrap().series[0].resume_episode_id, Some(eps[2].id));
        db.set_watched(eps[1].media_file_id, false).unwrap();
        assert_eq!(db.library().unwrap().series[0].resume_episode_id, Some(eps[0].id), "unmarking gives the half-played one back");
    }

    #[test]
    fn play_skips_an_episode_abandoned_before_later_ones_were_finished() {
        let eps = [ep(false, 600_000, Some(10)), ep(true, 0, Some(20)), ep(true, 0, Some(30)), ep(false, 0, None)];
        assert_eq!(pick_resume(&eps), Some(3));
    }

    #[test]
    fn specials_come_after_the_numbered_seasons() {
        let db = memory_db();
        db.import(&[proposed("a/s00e01.mkv", "Dark", 0, 1), proposed("a/s01e01.mkv", "Dark", 1, 1)]).unwrap();
        let lib = db.library().unwrap();
        let order: Vec<i64> = lib.series[0].seasons.iter().map(|s| s.number).collect();
        assert_eq!(order, vec![1, 0]);
        assert_eq!(lib.series[0].resume_episode_id, Some(lib.series[0].seasons[0].episodes[0].id), "Play starts S1 E1, not the special");
    }

    #[test]
    fn the_thumbnail_queue_can_step_past_a_file_and_retries_failures_next_launch() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2)]).unwrap();
        let (first, _) = db.next_unprobed(0).unwrap();
        let (second, _) = db.next_unprobed(first).unwrap();
        assert_ne!(first, second);
        assert_eq!(db.next_unprobed(second), None);
        db.set_probed(first, false, 0, 0, 0);
        assert_eq!(db.next_unprobed(0).map(|(id, _)| id), Some(second), "a failed file is not retried in the same run");
    }

    #[test]
    fn removing_a_series_leaves_other_series_alone() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("b/1.mkv", "Severance", 1, 1)]).unwrap();
        let dark = db.library().unwrap().series.iter().find(|s| s.title == "Dark").unwrap().id;
        db.remove("series", dark).unwrap();
        let lib = db.library().unwrap();
        let titles: Vec<&str> = lib.series.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["Severance"]);
        assert_eq!(lib.series[0].seasons[0].episodes.len(), 1);
    }

    #[test]
    fn resume_backs_up_three_seconds_and_restarts_finished_items() {
        let hour = 3_600_000;
        assert_eq!(resume_position_ms(600_000, hour), 597_000);
        assert_eq!(resume_position_ms(hour - 60_000, hour), 0, "stopped in the credits");
        assert_eq!(resume_position_ms(4_000, hour), 0);
        assert_eq!(resume_position_ms(600_000, 0), 597_000, "length not known yet");
    }

    #[test]
    fn play_starts_the_first_episode_of_an_untouched_series() {
        assert_eq!(pick_resume(&[ep(false, 0, None), ep(false, 0, None)]), Some(0));
        assert_eq!(pick_resume(&[]), None);
    }

    #[test]
    fn play_prefers_the_most_recently_played_unfinished_episode() {
        let eps = [ep(true, 0, Some(10)), ep(false, 5_000, Some(20)), ep(false, 9_000, Some(30)), ep(false, 0, None)];
        assert_eq!(pick_resume(&eps), Some(2));
    }

    #[test]
    fn play_continues_after_the_last_watched_episode() {
        let eps = [ep(true, 0, Some(1)), ep(false, 0, None), ep(true, 0, Some(2)), ep(false, 0, None)];
        assert_eq!(pick_resume(&eps), Some(3));
    }

    #[test]
    fn a_fully_watched_series_starts_over() {
        assert_eq!(pick_resume(&[ep(true, 0, Some(1)), ep(true, 0, Some(2))]), Some(0));
    }

    #[test]
    fn import_merges_seasons_into_one_series_and_skips_known_files() {
        let db = memory_db();
        assert_eq!(db.import(&[proposed("a/s01e01.mkv", "Mr. Robot", 1, 1), proposed("a/s01e02.mkv", "Mr. Robot", 1, 2)]).unwrap(), 2);
        // Later import: same series spelled differently, a new season, and one file already known.
        assert_eq!(db.import(&[proposed("b/s02e01.mkv", "mr robot", 2, 1), proposed("a/s01e02.mkv", "Mr. Robot", 1, 2)]).unwrap(), 1);
        let lib = db.library().unwrap();
        assert_eq!(lib.series.len(), 1);
        assert_eq!(lib.series[0].title, "Mr. Robot");
        let counts: Vec<(i64, usize)> = lib.series[0].seasons.iter().map(|s| (s.number, s.episodes.len())).collect();
        assert_eq!(counts, vec![(1, 2), (2, 1)]);
    }

    #[test]
    fn progress_is_saved_and_drives_the_resume_episode() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2)]).unwrap();
        let first = db.library().unwrap().series[0].seasons[0].episodes[0].media_file_id;
        db.save_progress(first, 3_500_000, 3_600_000).unwrap();
        let lib = db.library().unwrap();
        let eps = &lib.series[0].seasons[0].episodes;
        assert!(eps[0].watched);
        assert_eq!(lib.series[0].resume_episode_id, Some(eps[1].id));
        assert_eq!(db.media(first).unwrap().1, 3_500_000);
    }

    #[test]
    fn moving_and_removing_episodes_keeps_the_rest() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2)]).unwrap();
        let id = db.library().unwrap().series[0].seasons[0].episodes[1].id;
        db.move_episode(id, 2, Some(1)).unwrap();
        let lib = db.library().unwrap();
        assert_eq!(lib.series[0].seasons.len(), 2);
        assert_eq!(lib.series[0].seasons[1].episodes[0].id, id);
        db.remove("episode", id).unwrap();
        assert_eq!(db.library().unwrap().series[0].seasons[1].episodes.len(), 0);
        db.remove("series", lib.series[0].id).unwrap();
        assert!(db.library().unwrap().series.is_empty());
    }

    fn entry(season: i64, number: i64, after: Option<(i64, Option<i64>)>) -> OrderEntry {
        OrderEntry { season, number, after }
    }

    #[test]
    fn unplaced_specials_are_watched_last() {
        let entries = [entry(1, 1, None), entry(1, 2, None), entry(0, 1, None)];
        assert_eq!(watch_order(&entries), vec![0, 1, 2]);
    }

    #[test]
    fn a_placed_special_plays_right_after_its_episode() {
        // S1 E1, S1 E2, S2 E1, then a special that airs after S1 E1.
        let entries = [entry(1, 1, None), entry(1, 2, None), entry(2, 1, None), entry(0, 1, Some((1, Some(1))))];
        assert_eq!(watch_order(&entries), vec![0, 3, 1, 2]);
    }

    #[test]
    fn a_special_can_open_a_season_or_close_it() {
        let entries = [entry(1, 1, None), entry(2, 1, None), entry(2, 2, None), entry(0, 1, Some((2, Some(0)))), entry(0, 2, Some((1, None)))];
        assert_eq!(watch_order(&entries), vec![0, 4, 3, 1, 2], "the one closing S1 plays before the one opening S2");
    }

    #[test]
    fn a_special_after_a_missing_episode_follows_the_last_one_before_the_gap() {
        let entries = [entry(1, 1, None), entry(1, 4, None), entry(0, 1, Some((1, Some(3))))];
        assert_eq!(watch_order(&entries), vec![0, 2, 1]);
    }

    #[test]
    fn a_special_placed_in_a_season_that_is_gone_goes_last() {
        let entries = [entry(1, 1, None), entry(0, 1, Some((5, None)))];
        assert_eq!(watch_order(&entries), vec![0, 1]);
    }

    #[test]
    fn play_and_next_follow_the_placed_special() {
        let db = memory_db();
        let mut ova = proposed("a/ova.mkv", "Mushoku", 0, 1);
        ova.after_season = Some(1);
        db.import(&[proposed("a/1.mkv", "Mushoku", 1, 1), proposed("a/2.mkv", "Mushoku", 2, 1), ova]).unwrap();
        let lib = db.library().unwrap();
        let ids = |season: usize| lib.series[0].seasons[season].episodes[0].id;
        assert_eq!(lib.series[0].watch_order, vec![ids(0), ids(2), ids(1)], "the OVA plays between the seasons");
        db.save_progress(lib.series[0].seasons[0].episodes[0].media_file_id, 3_590_000, 3_600_000).unwrap();
        assert_eq!(db.library().unwrap().series[0].resume_episode_id, Some(ids(2)), "after S1 comes the OVA, not S2");
    }

    #[test]
    fn moving_an_episode_into_specials_keeps_its_place_in_watch_order() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2), proposed("a/3.mkv", "Dark", 1, 3)]).unwrap();
        let before = db.library().unwrap().series[0].watch_order.clone();
        db.move_episode(before[1], 0, None).unwrap();
        let lib = db.library().unwrap();
        assert_eq!(lib.series[0].watch_order, before, "the recap still plays second");
        let moved = &lib.series[0].seasons[1].episodes[0];
        assert_eq!((moved.season_number, moved.number, moved.after_season, moved.after_number), (0, 1, Some(1), Some(2)));
        db.move_episode(moved.id, 1, Some(2)).unwrap();
        let back = db.library().unwrap().series[0].seasons[0].episodes[1].clone();
        assert_eq!((back.id, back.after_season, back.after_number), (moved.id, None, None), "moved back, placement is dropped");
    }

    #[test]
    fn placing_a_special_by_hand_and_sending_it_back_to_the_end() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/2.mkv", "Dark", 1, 2), proposed("a/sp.mkv", "Dark", 0, 1)]).unwrap();
        let lib = db.library().unwrap();
        let (e1, e2, sp) = (lib.series[0].seasons[0].episodes[0].id, lib.series[0].seasons[0].episodes[1].id, lib.series[0].seasons[1].episodes[0].id);
        db.place_special(sp, Some(1), Some(0)).unwrap();
        assert_eq!(db.library().unwrap().series[0].watch_order, vec![sp, e1, e2]);
        db.place_special(sp, None, Some(4)).unwrap();
        let after = db.library().unwrap();
        assert_eq!(after.series[0].watch_order, vec![e1, e2, sp]);
        assert_eq!(after.series[0].seasons[1].episodes[0].after_number, None, "no season means no episode either");
    }

    #[test]
    fn only_specials_can_be_placed() {
        let db = memory_db();
        db.import(&[proposed("a/1.mkv", "Dark", 1, 1), proposed("a/sp.mkv", "Dark", 0, 1)]).unwrap();
        let lib = db.library().unwrap();
        let (e1, sp) = (lib.series[0].seasons[0].episodes[0].id, lib.series[0].seasons[1].episodes[0].id);
        assert_eq!(db.place_special(e1, Some(1), None), Err("Only specials can be placed between episodes. Move it to Specials first.".into()));
        assert_eq!(db.place_special(sp, Some(0), None), Err("A special can't be placed among the specials. Pick a numbered season.".into()));
        assert!(db.move_episode(e1, -1, None).is_err());
    }

    #[test]
    fn a_special_that_would_share_a_number_takes_the_next_free_one() {
        let db = memory_db();
        db.import(&[proposed("a/ova1.mkv", "Frieren", 0, 1), proposed("a/ova2.mkv", "Frieren", 0, 2)]).unwrap();
        db.import(&[proposed("b/sp1.mkv", "Frieren", 0, 1), proposed("b/1.mkv", "Frieren", 1, 1), proposed("b/1v2.mkv", "Frieren", 1, 1)]).unwrap();
        let lib = db.library().unwrap();
        let numbers = |n: i64| lib.series[0].seasons.iter().find(|s| s.number == n).unwrap().episodes.iter().map(|e| e.number).collect::<Vec<_>>();
        assert_eq!(numbers(0), vec![1, 2, 3]);
        assert_eq!(numbers(1), vec![1, 1], "numbered seasons keep the number the user gave");
    }

    #[test]
    fn a_library_from_the_second_version_gains_special_placement() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!("{SCHEMA_V1}{SCHEMA_V2}PRAGMA user_version = 2;")).unwrap();
        let db = Db::with_connection(conn, std::env::temp_dir().join("marquee-test-thumbs")).unwrap();
        db.import(&[proposed("a/sp.mkv", "Dark", 0, 1)]).unwrap();
        let version: i64 = db.conn.lock().unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!((version, db.library().unwrap().series[0].seasons[0].episodes[0].after_season), (3, None));
    }

    #[test]
    fn renaming_rejects_empty_names() {
        let db = memory_db();
        let id = db.create_series("Untitled").unwrap();
        assert!(db.rename("series", id, "   ").is_err());
        db.rename("series", id, "Severance").unwrap();
        assert_eq!(db.library().unwrap().series[0].title, "Severance");
    }
}
