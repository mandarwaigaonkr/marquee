mod db;
mod organiser;
mod player;
mod thumbs;

use db::{Db, Library, WatchFolder};
use organiser::Proposed;
use player::{Player, PlayerState};
use thumbs::Previews;
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

type Res<T> = Result<T, String>;

// Commands marked (async) touch the disk, so they run off the main thread: a
// sleeping drive must not freeze the window.

// ---- library ----

#[tauri::command(async)]
fn library(db: State<Db>) -> Res<Library> {
    db.library()
}

#[tauri::command(async)]
fn import_scan(paths: Vec<String>) -> Vec<Proposed> {
    organiser::scan(&paths)
}

#[tauri::command(async)]
fn import_commit(db: State<Db>, items: Vec<Proposed>) -> Res<usize> {
    db.import(&items)
}

#[tauri::command]
fn rename_item(db: State<Db>, kind: String, id: i64, title: String) -> Res<()> {
    db.rename(&kind, id, &title)
}

#[tauri::command(async)]
fn remove_item(db: State<Db>, kind: String, id: i64) -> Res<()> {
    db.remove(&kind, id)
}

#[tauri::command]
fn create_series(db: State<Db>, title: String) -> Res<i64> {
    db.create_series(&title)
}

#[tauri::command]
fn add_season(db: State<Db>, series_id: i64) -> Res<i64> {
    db.add_season(series_id)
}

#[tauri::command]
fn move_episode(db: State<Db>, episode_id: i64, season_number: i64, number: Option<i64>) -> Res<()> {
    db.move_episode(episode_id, season_number, number)
}

#[tauri::command]
fn place_special(db: State<Db>, episode_id: i64, after_season: Option<i64>, after_number: Option<i64>) -> Res<()> {
    db.place_special(episode_id, after_season, after_number)
}

#[tauri::command]
fn set_watched(db: State<Db>, media_file_id: i64, watched: bool) -> Res<()> {
    db.set_watched(media_file_id, watched)
}

// ---- moved files ----

#[tauri::command(async)]
fn file_missing(db: State<Db>, media_file_id: i64) -> Res<bool> {
    Ok(!Path::new(&db.media(media_file_id)?.0).exists())
}

/// Points one library item at a file the user picked.
#[tauri::command(async)]
fn relink_file(db: State<Db>, media_file_id: i64, path: String) -> Res<()> {
    db.relink(&[(media_file_id, path)]).map(|_| ())
}

/// Looks in the given folders for library files that are no longer where they were.
/// Returns (how many were found again, how many are still missing).
#[tauri::command(async)]
fn relink_folder(db: State<Db>, paths: Vec<String>) -> Res<(usize, usize)> {
    let missing: Vec<(i64, String, i64)> = db.files()?.into_iter().filter(|(_, path, _)| !Path::new(path).exists()).collect();
    if missing.is_empty() {
        return Ok((0, 0));
    }
    let found: Vec<(String, i64)> = organiser::video_files(&paths)
        .iter()
        .map(|p| (p.to_string_lossy().into_owned(), fs::metadata(p).map(|m| m.len() as i64).unwrap_or(0)))
        .collect();
    let done = db.relink(&db::match_moved(&missing, &found))?;
    Ok((done, missing.len() - done))
}

// ---- watch folders ----

/// A file changed in the last minute may still be downloading or copying.
const SETTLE: Duration = Duration::from_secs(60);
const WATCH_EVERY: Duration = Duration::from_secs(30);

/// Adds whatever is new in a watch folder, sorted the usual way. Files the organiser
/// can't place are left for the user to add by hand.
fn sweep(db: &Db, folder: &str) -> usize {
    let settled = |path: &str| {
        fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|at| SystemTime::now().duration_since(at).is_ok_and(|age| age >= SETTLE))
    };
    let found: Vec<Proposed> = organiser::scan(&[folder.to_owned()]).into_iter().filter(|p| p.include && settled(&p.path)).collect();
    db.import_watched(&found).unwrap_or(0)
}

#[tauri::command]
fn watch_folders(db: State<Db>) -> Res<Vec<WatchFolder>> {
    db.watch_folders()
}

/// Starts watching a folder and adds what is in it now. Returns how many files were added.
#[tauri::command(async)]
fn add_watch_folder(db: State<Db>, path: String) -> Res<usize> {
    if !Path::new(&path).is_dir() {
        return Err(format!("{path} is not a folder Marquee can open. Check that its drive is connected, then add it again."));
    }
    db.add_watch_folder(&path)?;
    Ok(sweep(&db, &path))
}

#[tauri::command]
fn remove_watch_folder(db: State<Db>, id: i64) -> Res<()> {
    db.remove_watch_folder(id)
}

fn watch(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(WATCH_EVERY);
        // Not while something plays: the disk and the database belong to playback then.
        if app.state::<Player>().current().is_some() {
            continue;
        }
        let db = app.state::<Db>();
        let added: usize = db.watch_folders().unwrap_or_default().iter().map(|f| sweep(&db, &f.path)).sum();
        if added > 0 {
            let _ = app.emit("library-changed", ());
            let _ = app.emit("watch-added", added);
        }
    });
}

// ---- playback ----

fn save_progress(db: &Db, player: &Player) {
    if let Some((id, position, duration)) = player.progress() {
        let _ = db.save_progress(id, position, duration);
    }
}

#[tauri::command(async)]
fn player_play(db: State<Db>, player: State<Player>, media_file_id: i64, from_start: bool) -> Res<bool> {
    // False means a newer play or a stop overtook this one while it was looking the file up.
    let turn = player.begin();
    save_progress(&db, &player);
    let (path, position_ms, _, duration_ms) = db.media(media_file_id)?;
    let start = if from_start { 0 } else { db::resume_position_ms(position_ms, duration_ms) };
    player.load(turn, media_file_id, &path, start, db.track_prefs(media_file_id))
}

#[tauri::command]
fn player_stop(db: State<Db>, player: State<Player>) -> Res<()> {
    save_progress(&db, &player);
    player.stop()
}

#[tauri::command]
fn player_toggle_pause(db: State<Db>, player: State<Player>) -> Res<()> {
    save_progress(&db, &player);
    player.toggle_pause()
}

#[tauri::command]
fn player_seek(player: State<Player>, seconds: f64, absolute: bool) -> Res<()> {
    player.seek(seconds, absolute)
}

#[tauri::command]
fn player_set_track(db: State<Db>, player: State<Player>, kind: String, id: Option<i64>) -> Res<()> {
    player.set_track(&kind, id)?;
    // The choice is remembered for the rest of the series, by language.
    let lang = match id {
        Some(id) => player.track_lang(&kind, id),
        None => Some("no".to_owned()),
    };
    if let (Some(media_file_id), Some(lang)) = (player.current(), lang) {
        db.set_track_pref(media_file_id, &kind, &lang)?;
    }
    Ok(())
}

#[tauri::command]
fn player_set_number(player: State<Player>, property: String, value: f64) -> Res<()> {
    player.set_number(&property, value)
}

#[tauri::command]
fn player_add_subtitle(player: State<Player>, path: String) -> Res<()> {
    player.add_subtitle(&path)
}

/// How far apart scrubber frames are. Each one is made once and kept.
const PREVIEW_STEP_SECONDS: f64 = 10.0;

/// Path of a small frame near `seconds`, for the scrubber preview.
#[tauri::command(async)]
fn preview_frame(db: State<Db>, previews: State<Previews>, media_file_id: i64, seconds: f64) -> Res<String> {
    let step = (seconds / PREVIEW_STEP_SECONDS).round().max(0.0) as i64;
    let out = db.preview_path(media_file_id, step);
    if !out.exists() && !previews.frame(&db.media(media_file_id)?.0, step as f64 * PREVIEW_STEP_SECONDS, &out) {
        return Err("No preview frame for that moment.".into());
    }
    Ok(out.to_string_lossy().into_owned())
}

#[tauri::command]
fn player_state(player: State<Player>) -> PlayerState {
    player.state()
}

#[tauri::command]
fn toggle_fullscreen(window: WebviewWindow) -> Res<bool> {
    let fullscreen = !window.is_fullscreen().map_err(|e| e.to_string())?;
    window.set_fullscreen(fullscreen).map_err(|e| e.to_string())?;
    Ok(fullscreen)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let window = app.get_webview_window("main").ok_or("main window missing")?;
            let mut data_dir = app.path().app_data_dir()?;
            // Development builds can point at a throwaway library and fill it at launch:
            // MARQUEE_DATA_DIR=<folder>  MARQUEE_DEV_IMPORT=<path>;<path>
            if cfg!(debug_assertions) {
                if let Ok(dir) = std::env::var("MARQUEE_DATA_DIR") {
                    data_dir = dir.into();
                }
            }
            let db = Db::open(&data_dir)?;
            // The webview may read thumbnails, and nothing else, from disk.
            app.asset_protocol_scope().allow_directory(data_dir.join("thumbs"), false)?;
            if cfg!(debug_assertions) {
                if let Ok(paths) = std::env::var("MARQUEE_DEV_IMPORT") {
                    let paths: Vec<String> = paths.split(';').map(str::to_owned).collect();
                    db.import(&organiser::scan(&paths))?;
                }
            }
            app.manage(db);
            app.manage(Player::new(window.hwnd()?.0 as isize as i64)?);

            // Progress is saved every five seconds while something plays.
            let handle = app.handle().clone();
            thread::spawn(move || loop {
                thread::sleep(Duration::from_secs(5));
                save_progress(&handle.state::<Db>(), &handle.state::<Player>());
            });

            // The thumbnail worker renders into a window that is never shown.
            // It gets a blank page rather than a second copy of the app.
            let hidden = WebviewWindowBuilder::new(app, "thumbs", WebviewUrl::External("about:blank".parse()?))
                .visible(false)
                .skip_taskbar(true)
                .build()?;
            app.manage(thumbs::spawn(app.handle().clone(), hidden.hwnd()?.0 as isize as i64, data_dir.join("thumbs").join("making")));
            watch(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::CloseRequested { .. }) {
                let app = window.app_handle();
                save_progress(&app.state::<Db>(), &app.state::<Player>());
                // Closing the main window ends the app, hidden worker window included.
                app.exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            library,
            import_scan,
            import_commit,
            rename_item,
            remove_item,
            create_series,
            add_season,
            move_episode,
            place_special,
            set_watched,
            file_missing,
            relink_file,
            relink_folder,
            watch_folders,
            add_watch_folder,
            remove_watch_folder,
            player_play,
            player_stop,
            player_toggle_pause,
            player_seek,
            player_set_track,
            player_set_number,
            player_add_subtitle,
            player_state,
            preview_frame,
            toggle_fullscreen
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    /// The version lives in three files; an installer must not claim a different one than the app.
    #[test]
    fn every_file_carries_the_same_version() {
        let version_in = |json: &str| -> String {
            let value: serde_json::Value = serde_json::from_str(json).unwrap();
            value["version"].as_str().unwrap().to_string()
        };
        let cargo = env!("CARGO_PKG_VERSION");
        assert_eq!(version_in(include_str!("../tauri.conf.json")), cargo, "tauri.conf.json");
        assert_eq!(version_in(include_str!("../../package.json")), cargo, "package.json");
    }
}
