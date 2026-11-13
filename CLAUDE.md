# Marquee

A Windows desktop library and player for locally downloaded video files. It sorts files into series, seasons and episodes from their names, lets the user fix anything by hand, resumes where they stopped and rolls into the next episode. It replaces VLC for one person's own collection.

Tauri 2 (Rust) + React 19 / TypeScript / Vite + libmpv. Read `docs/ARCHITECTURE.md` before changing how the pieces talk to each other, and `docs/DESIGN.md` before touching anything visual.

## Commands

Run from the repo root. In Git Bash, put cargo on the path first: `export PATH="$HOME/.cargo/bin:$PATH"`.

| Task | Command |
|---|---|
| Run the app | `npm run tauri dev` |
| Rust tests | `cd src-tauri && cargo test` |
| Type-check the UI | `npx tsc --noEmit` |
| Production build | `npm run tauri build` |

Development builds read two environment variables so testing never touches the real library:

- `MARQUEE_DATA_DIR=<folder>` puts the database and thumbnails there instead of `%APPDATA%\com.mandar.marquee`.
- `MARQUEE_DEV_IMPORT=<path>;<path>` scans and imports those paths at launch.

To drive the running webview from a script, launch with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222 --remote-allow-origins=*"` and connect over the DevTools protocol. The Browser pane cannot run the app: the UI needs the Tauri backend.

## Setup facts that are not obvious

- `vendor/libmpv/libmpv-2.dll` and `mpv.lib` are not in git. `docs/ARCHITECTURE.md` says how to fetch the DLL and generate the import library.
- Windows Smart App Control must be off to build (it blocks unsigned build scripts, `os error 4551`).

## Rules that always apply

- **The user's video files are read-only.** Never move, rename, copy or delete them. "Remove" only ever deletes library rows and cached thumbnails.
- **Manual edits win.** The organiser proposes; nothing it does may overwrite a name or placement the user changed.
- **Playback must not stutter.** Nothing on the playback path may wait on the database, the disk scan or the thumbnail worker.
- **Design is locked**: true black, square corners, no gradients, monochrome, one expanding gallery (Library only). See `.claude/rules/design.md`.
- **The repo is public** at `github.com/mandarwaigaonkr/marquee` (MIT). Push only when the user asks, and run `gh auth status` first (personal account `mandarwaigaonkr`). Commits use the GitHub no-reply address set in this repo's git config.
- **Nothing from copyrighted shows goes in the repo**: no frames, thumbnails or screenshots of them. Screenshots use the Blender open films.

Scoped rules live in `.claude/rules/` (`design.md`, `frontend.md`, `backend.md`). Review agents live in `.claude/agents/`.

## Agents

| Agent | Use it for |
|---|---|
| `frontend-griller` | Adversarial review of `src/` against the design and UX rules, React correctness and keyboard access. |
| `backend-griller` | Adversarial review of `src-tauri/src/` for data loss, locking, mpv misuse, organiser mistakes. |
| `organiser-tester` | Throw real-world filenames at the organiser and add failing cases as tests. |

Grillers only report. Verify a finding (read the cited lines or run a test) before acting on it.
