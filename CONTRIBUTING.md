# Contributing to Marquee

Thank you for looking. Marquee is small and young, so one person can make a visible difference. This page tells you how to set up, where things are, and what a change must satisfy to be merged.

## Ways to help

| You have | Do this |
|---|---|
| A file name Marquee sorted wrong | Open a [file name issue](https://github.com/mandarwaigaonkr/marquee/issues/new?template=filename.yml). No code needed. |
| A file that will not play, or plays badly | Open a [bug report](https://github.com/mandarwaigaonkr/marquee/issues/new?template=bug.yml) with the stats overlay (press I while playing). |
| An idea | Open a [feature request](https://github.com/mandarwaigaonkr/marquee/issues/new?template=feature.yml). Say what you are trying to do, not only the feature. |
| Time to write code | Pick an issue labelled `good first issue` or `help wanted`, comment that you are taking it, and read on. |

Good first changes:

- **Organiser patterns.** `src-tauri/src/organiser.rs` is a pure function from a path to a proposal, with a test per pattern. Add a failing test with a real file name, then make it pass.
- **Items under "Not yet" in the README.** Merging two series, moving a film into a series, intro skip from chapters.
- **A Linux build.** Tauri and mpv both run there; the transparent-window layering needs a different approach on Wayland.

For anything larger than a bug fix, open an issue first so we can agree on the approach before you spend the time.

## Set up

You need Windows 11, Rust with the MSVC toolchain, and Node 20 or newer.

1. Fork and clone the repository.
2. Fetch `libmpv-2.dll` and generate `mpv.lib` into `vendor/libmpv/`. The steps are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). They are not in git.
3. Install and run:

```bash
npm install
```

```bash
npm run tauri dev
```

Keep your own library out of development. Set `MARQUEE_DATA_DIR` to a scratch folder before starting, and the database and thumbnails go there instead of `%APPDATA%\com.mandar.marquee`.

If the build fails with `os error 4551`, Windows Smart App Control is blocking unsigned build scripts and has to be turned off.

## Before you open a pull request

```bash
cd src-tauri && cargo test
```

```bash
npx tsc --noEmit
```

Both must pass, with no new warnings. Then:

- New logic has a test. Organiser patterns get a test named for the behaviour, using a real file name.
- One change per pull request. Say what it does and how you checked it; add a screenshot or short clip for anything visible.
- Interface changes follow [docs/DESIGN.md](docs/DESIGN.md).

## Rules the project holds to

These are not preferences; a change that breaks one will not be merged.

1. **The user's video files are read-only.** Never move, rename, copy or delete them.
2. **Manual edits win.** Nothing automatic may overwrite a name or placement the user changed.
3. **Playback must not stutter.** Nothing on the playback path waits on the database, a disk scan or the thumbnail worker. Check changes near playback against a 4K HDR file and the dropped-frame count in the stats overlay.
4. **The design is fixed**: true black, square corners, no gradients, monochrome, and motion on `transform` and `opacity` only.
5. **A shipped database schema is never edited.** Changes are a new step behind `PRAGMA user_version`, so existing libraries survive upgrades.

The longer versions are in `.claude/rules/` (`backend.md`, `frontend.md`, `design.md`). They are written for an AI assistant but they are the project's real rules, and they read fine for people.

## Using AI assistants

Welcome, and the repository is set up for it (`CLAUDE.md`, `.claude/`). You are responsible for what you submit: run it, test it, and be able to explain it.

## Conduct

Be kind and specific. Criticise code, not people. Maintainers may remove comments or contributors that make the project unpleasant to work on.

## Licence

By contributing you agree that your work is released under the [MIT licence](LICENSE).
