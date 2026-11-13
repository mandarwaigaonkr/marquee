## Always

## Lessons
- cargo build fails "os error 4551": Windows Smart App Control blocks unsigned build scripts; must be Off to build.
- vendor/libmpv: official mpv 0.42.0 x86_64 lgpl zip; mpv.lib is generated from the DLL via dumpbin + lib /def (mpv.def).
- Design: #000, radius 0, no gradients, real frames; accordion ONLY in Library; transform-only motion (design/mock/index.html).
- Test without touching the real library: MARQUEE_DATA_DIR + MARQUEE_DEV_IMPORT (debug only); drive webview via CDP port 9222.
- tauri WindowBuilder needs the `unstable` feature; the hidden thumbs window is a WebviewWindow on about:blank instead.
- organiser JUNK: plain words like "complete" cut real titles ("The Complete ... Course"); only tag-shaped tokens belong there.
- The Rust `regex` crate has no lookahead. To skip a rejected match and keep looking, loop with `captures_at` from the end of the rejected group (see the anime branch in `organiser.rs`).
- mpv posts a file's end-of-file event a moment after `stop` returns. Wait for `idle-active` before draining events, or the next load reads it as its own failure (`thumbs.rs`).
- Scrubber previews must not use a GPU mpv: a second gpu-next cost ~50 dropped frames per scrub on 4K HDR. `thumbs.rs` uses vo=image.
- `showModal()` focuses the first button and ignores React `autoFocus` on a later one; focus it by hand after (`MissingFile` in `ui.tsx`).
- Driving the UI by script: React pointer enter/leave only fire from `pointerover`/`pointerout` with relatedTarget set.
