---
paths:
  - "src-tauri/**"
---

# Backend rules

- `organiser.rs` is pure: path in, `Proposed` out, no database, no mpv. Every filename pattern it learns gets a test named for the behaviour, using a real filename.
- `db.rs` owns all SQL. The rules that decide what plays next (`is_watched`, `resume_position_ms`, `pick_resume`) are pure functions with tests; keep them that way.
- Schema changes are a new `SCHEMA_Vn` step behind `PRAGMA user_version`. Never edit a shipped schema string: the user's library must survive upgrades.
- Never write to, move, rename or delete a path that came from the user's library. The only files the app deletes are its own thumbnails.
- Commands in `lib.rs` are thin: take state, call one module, return `Result<T, String>`. The string is shown to the user as is, so it says what happened and what to do next.
- The database connection sits behind one `Mutex`. Do not hold it while calling mpv or doing file IO that can block (a sleeping external drive can stall for seconds).
- Three mpv instances exist: the player (in the main window), the thumbnail grabber (GPU, in a hidden window) and the scrubber previewer (CPU only, `vo=image`). The thumbnail grabber stands down while the player has something loaded. The previewer runs during playback, so it must stay off the GPU: measure dropped frames on a 4K HDR file before changing it. Do not share instances.
- A watch folder may only add files. It never re-adds a path in `removed_paths`, never touches items that exist, and never runs while something plays.
- mpv properties are read with a fallback, never unwrapped: most are unavailable while nothing is loaded.
- Player options that affect compatibility or performance (`vo`, `hwdec`, subtitle options) are changed only with a reason in a comment, and re-checked against a 4K HDR file.
- `cargo test` must pass, with no new warnings.
