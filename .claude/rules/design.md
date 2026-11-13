---
paths:
  - "src/**"
  - "design/**"
---

# Design rules

The approved reference is `design/mock/index.html`. The user rejected three earlier directions; these rules are what survived.

## Never

- Rounded corners. `* { border-radius: 0 }` stays.
- Gradients, scrims that fade, glows, glass blur, drop shadows for depth.
- Colour other than the tokens in `src/styles/app.css`. Colour comes from the video frames; the UI is black, white and greys. `--danger` is for destructive actions and errors only.
- A second expanding gallery. The accordion is the Library section's signature and appears nowhere else. Everything else is plain rows, a feature block, tabs.
- Netflix patterns: hero banner, horizontal poster carousels.
- Generated or stock artwork. Thumbnails are frames grabbed from the user's own files; a missing one is a flat `--surface-2` box.

## Always

- Tokens only: `--canvas` `--surface` `--surface-2` `--line` `--line-strong` `--text` `--text-secondary` `--accent`. Depth is a 1px line, not a shadow.
- Type: Geist, weights 400/500/600, sizes 12/14/16/20/28/48px (the `--text-*` tokens). Sentence case.
- Spacing on the 4/8px grid. Hit targets at least 44px.
- Motion animates `transform` and `opacity` only, with `--fast` (150ms), `--base` (200ms), `--slide` (300ms) and `--ease`. Two helpers: `--tick` (250ms, linear) is only for the player's progress bar gliding between readings, `--stagger` is the gap between rows arriving. Lists and sections arrive with `rise` and `backwards` fill, so no transform is left behind to trap a menu. Hover washes that fade are a pseudo-element's opacity, not a background transition. Never animate width, height, flex-basis, left/top or filter: the first gallery did and it stuttered.
- `prefers-reduced-motion` keeps working (the block at the end of `app.css`).
- Icon-only buttons carry `data-tip` and `aria-label`.
- Copy is verb-first and specific ("Resume S2 E5", "Add files", "Remove Suits"). Errors say what happened and what to do next, and never blame the user.
- Destructive actions confirm in a dialog that names the thing and says the files stay on disk.
- Renaming happens in place with `InlineEdit`; the row must not move.

## Before calling UI work done

Run the VectorLab `ux-audit` skill on the changed screens with real screenshots, plus `frontend-design` and the relevant `vectorlab-ux-skills:*` skills when building something new. The user expects the full skill pass, not a summary of it.
