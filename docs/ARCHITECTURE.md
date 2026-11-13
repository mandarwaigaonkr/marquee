# Architecture

## The shape

```
┌──────────────────────── main window ────────────────────────┐
│  WebView2 (transparent)   React UI: library, controls       │
│  ─────────────────────────────────────────────────────────  │
│  mpv child window         video, subtitles (behind the UI)  │
└──────────────────────────────────────────────────────────────┘
        │ invoke()                       ▲ events ("library-changed")
        ▼                                │
┌──────────────────────── Rust (src-tauri/src) ───────────────┐
│ lib.rs        commands, setup, progress-saving thread        │
│ organiser.rs  file path -> Proposed (series/season/episode)  │
│ db.rs         SQLite: library, progress, what plays next     │
│ player.rs     the playing mpv instance                       │
│ thumbs.rs     two more mpv instances: thumbnails, previews   │
└──────────────────────────────────────────────────────────────┘
```

The video never passes through the webview. mpv draws into a native child window of the main window (`wid`), and the webview on top is transparent. While something plays, the page background is switched off (`html.playing`) so the video shows through and React draws only the controls. That is why HDR, 4K and GPU decoding cost nothing extra.

## Backend modules

| File | Owns | Notes |
|---|---|---|
| `organiser.rs` | Turning a path into a `Proposed` | Pure. Patterns tried in order: `S01E02` (and `S01E01E02`), `1x02`, `Episode 12`, anime ` - 12`, then `12. Name` when the folder has siblings like it or is itself numbered. Series name is the text before the match, cleaned of release tags; if empty, it comes from the folders. No pattern: movie if the name has a year, otherwise unsorted and left out by default. |
| `db.rs` | All SQL, and the rules for watched, resume point and which episode Play starts | `is_watched`: 92% through or under 2 minutes left. Watched is sticky: only "Mark as unwatched" clears it. `resume_position_ms`: 3 s before the saved point; from the start if that point is in the credits or under 10 s in. `pick_resume`: most recently played unfinished episode (unless something was finished or hand-marked after it), else the first unwatched after the last watched, else the first unwatched anywhere, else the first. Play requests carry a turn number (`Player::begin`), so a slow one that was overtaken by a stop or a newer play does nothing. |
| `player.rs` | The mpv instance that plays | `vo=gpu-next`, `hwdec=auto-safe`, `keep-open` (so the end of a file is a state the UI can see), all mpv input and on-screen controls off. Remembers which media file is loaded so progress is saved against it. |
| `thumbs.rs` | Frames from the user's files, on one worker thread | **Thumbnails**: an mpv renders into a hidden window so frames go through the same tone-mapping as playback. Seeks 10% in, screenshots at 960 px wide, stores `<thumbs>/<media_file_id>.jpg`, reads duration and size. Stands down while the player has something loaded. Emits `library-changed`. **Scrubber previews**: a separate mpv with `vo=image`, CPU decoding on two threads and keyframe seeks writes 320 px frames to `<thumbs>/making/`, which are moved to `<thumbs>/p<media_file_id>_<step>.jpg` (one step per 10 s) and kept. It serves requests during playback and never touches the GPU: a GPU renderer here cost a 4K HDR file about fifty dropped frames per scrub. Only the newest pending request is served; the file is let go after 20 s without one. |
| `lib.rs` | Tauri commands and wiring | Thin. A thread saves progress every 5 s; progress is also saved on pause, stop, switching file and window close. Another thread sweeps the watch folders every 30 s (not while something plays), skipping files changed in the last minute. |

## Data

SQLite at `%APPDATA%\com.mandar.marquee\library.db` (WAL), thumbnails beside it in `thumbs\`.

| Table | Holds |
|---|---|
| `media_files` | One row per file on disk: path (unique), size, duration, dimensions, thumbnail state (0 pending, 1 done, 2 failed) |
| `series` | Title and `match_key` (lower-case letters and digits of the title; unique). Imports merge into a series by key, and renaming a series updates it. |
| `seasons` | `(series_id, number)` unique, optional custom title |
| `episodes` | Season, number, optional end number for double episodes, title, media file |
| `movies` | Title, year, media file |
| `playback` | Per media file: position, watched, last played |
| `series_prefs` | Per series: remembered audio language and subtitle language (`no` = subtitles off) |
| `watch_folders` | Folders swept for new files |
| `removed_paths` | Paths the user removed from the library, so a watch folder does not bring them back. Adding one by hand clears it. |

Files are referenced in place. Removing anything deletes rows and cached thumbnails only.

Schema changes go in as a new step keyed on `PRAGMA user_version` (currently 2). Shipped schema strings are never edited.

**Track memory.** Choosing an audio or subtitle track stores its language for the series. Each load sets mpv's `alang`/`slang` from that and resets `aid`/`sid` to `auto` (or `sid=no`), so a track picked by number in one file is never applied to the next.

**Moved files.** Nothing is fingerprinted. `relink_folder` lists library files whose path no longer exists, walks the chosen folders, and pairs them with `match_moved`: same file name and size first, then same size when exactly one file on each side has it. `relink_file` points one item at a file picked by hand. Names, placement and progress are untouched; the thumbnail is remade.

## Commands

| Command | Does |
|---|---|
| `library` | Whole library, with each series' resume episode |
| `import_scan(paths)` | Proposals for files and folders; saves nothing |
| `import_commit(items)` | Saves the reviewed proposals; returns how many were new |
| `rename_item(kind, id, title)` | `series`, `season`, `episode`, `movie` |
| `remove_item(kind, id)` | `series`, `episode`, `movie` |
| `create_series`, `add_season`, `move_episode`, `set_watched` | Manual organising |
| `file_missing(media_file_id)` | Whether the file is still at its path |
| `relink_file(media_file_id, path)`, `relink_folder(paths)` | Point library items at where their files are now. The second returns (found, still missing). |
| `watch_folders`, `add_watch_folder(path)`, `remove_watch_folder(id)` | Adding sweeps the folder at once and returns how many files it added |
| `preview_frame(media_file_id, seconds)` | Path of a cached scrubber frame near that moment, made on demand |
| `player_play(media_file_id, from_start)` | Saves the old file's progress, loads the new one at its resume point |
| `player_stop`, `player_toggle_pause`, `player_seek`, `player_set_track`, `player_set_number`, `player_add_subtitle` | Playback control. `player_set_number` accepts `volume`, `sub-delay`, `sub-scale`, `sub-pos`, `speed`. `player_set_track` also stores the choice for the series. `player_play` returns false when a newer play or a stop overtook it. |
| `player_state` | Position, duration, paused, ended, tracks, decoder facts. Polled every 250 ms while playing. |
| `toggle_fullscreen` | |

## Frontend

| File | Role |
|---|---|
| `src/api.ts` | Types mirroring the Rust structs, one wrapper per command, derived facts (`playLabel`, `runtime`) |
| `src/App.tsx` | View state (home, series, import review, settings), library loading, drag and drop, Ctrl+K, notices, the missing-file dialog, focus hand-back after playback |
| `components/Home.tsx` | Continue watching, Library gallery, movies, empty state |
| `components/Gallery.tsx` | The expanding gallery. Panels are absolutely positioned at full open width and move by `transform` only. |
| `components/SeriesPage.tsx` | Season tabs with one mark per episode (the season map), episode rows, rename/move/mark/locate/remove |
| `components/Settings.tsx` | Watch folders, finding moved files, the shortcut list |
| `components/ImportReview.tsx` | Editable table of what the organiser found |
| `components/Player.tsx` | Controls over the video, audio and subtitle menu, episode list, shortcuts, next-episode countdown, frame preview on the scrubber (one request in flight, newest position wins) |
| `components/CommandBar.tsx` | Ctrl+K jump list |
| `components/ui.tsx` | Icons, icon button, inline rename, menu, confirmation dialog, missing-file dialog |
| `styles/app.css` | Tokens and every style |

Player shortcuts: Space or K play/pause, Left/Right 10 s, Up/Down volume, F fullscreen, M mute, N next episode, S audio and subtitles, A next audio track, [ and ] slower and faster, E episodes, I stats, Esc back. The list shown in Settings (`SHORTCUTS` in `Settings.tsx`) must match the handler in `Player.tsx`.

## Installer

`npm run tauri build -- --bundles nsis` writes `src-tauri/target/release/bundle/nsis/Marquee_<version>_x64-setup.exe`. `libmpv-2.dll` is bundled as a resource and lands beside the executable. The build is unsigned, so Windows SmartScreen warns on first run.

## Building libmpv for MSVC

The official mpv Windows development archive ships a MinGW import library only.

1. Download the mpv 0.42.0 `mpv-dev-x86_64` archive and put `libmpv-2.dll` and `include/` in `vendor/libmpv/`.
2. From a Visual Studio developer prompt, in `vendor/libmpv/`: `dumpbin -exports libmpv-2.dll`, turn the export names into `mpv.def` (already in the repo), then `lib -def:mpv.def -machine:x64 -out:mpv.lib`.

`src-tauri/build.rs` adds the folder to the link path and copies the DLL next to the built executable.

## Not built yet

Moving a movie into a series, merging two series, reordering by drag, subtitle style options beyond size and position, a second subtitle track, intro skip, online artwork lookup, code signing, an Ubuntu build.
