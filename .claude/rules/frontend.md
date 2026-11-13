---
paths:
  - "src/**"
---

# Frontend rules

- No router, no state library, no CSS framework. `App.tsx` holds the view and the library; components get props. Add a dependency only when it removes more code than it adds.
- All backend calls go through `src/api.ts`. Components never call `invoke` directly. Types there mirror the Rust structs field for field (snake_case); change both sides together.
- The library is loaded whole with `api.library()` and re-read after every change (`onChanged`). Do not keep a second copy of library data in component state.
- Derived facts (`playLabel`, `runtime`, `resumeEpisode`) live in `api.ts`, not inline in JSX.
- The player polls `player_state` every 250 ms. Do not add a second poller; pass state down.
- One stylesheet, `src/styles/app.css`, grouped by screen. No inline colours or sizes; inline `style` is only for values computed at runtime (transforms, the gallery's custom properties).
- Rules of Hooks, complete dependency arrays, cleanup for every listener and timer.
- A field cannot live inside a `<button>`. Rows that rename in place use `RowMain`.
- Overlays: native `<dialog>` for confirmation and the command bar (focus trap and restore for free); closed panels are `inert`.
- While a video plays, `html.playing` makes the page transparent so mpv shows through. Anything drawn over the video needs its own background.
- Check with `npx tsc --noEmit` before finishing.
