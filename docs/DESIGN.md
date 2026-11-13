# Design

The approved reference is `design/mock/index.html` (open it through a local server so the frames load). The short rules are in `.claude/rules/design.md`; this file is the reasoning.

## How it got here

| Tried | Verdict from the owner | What it taught |
|---|---|---|
| Violet ink with an amber accent | Rejected outright | Wants true-black OLED and a startup-tool feel, not a themed palette. |
| "Shelf" of vertical spines | "I don't like the shelf" | Asked for an expanding image gallery instead. |
| Expanding gallery used on every screen, rounded cards, gradient scrims | "AI slop 101", laggy | One signature element, used once. Square, flat, no gradients. Animation must be smooth. |
| Current: flat black, real frames, gallery in Library only | Approved | |

## Principles

1. **The frames are the colour.** The UI is black, white and three greys. Every image is a frame from the owner's own files, so the library looks like theirs and needs no posters.
2. **One signature.** The Library gallery expands the series under the pointer. Nothing else in the app behaves like it. Continue watching is a feature block and plain rows; a series is tabs and rows; the player is two flat bars.
3. **Flat and square.** Structure comes from 1px lines and three surface levels. No radius, shadow, blur or gradient anywhere.
4. **Motion you can't catch stuttering.** Only `transform` and `opacity` animate. The gallery lays every panel out at its open width and slides the ones to the right; opening a panel causes no layout. Measured at about 6 ms a frame in the mock.
5. **Says what it does.** "Resume S2 E5", "Remove Suits", "Add files". Errors explain and offer the next step.

## Tokens

| Token | Value | Use |
|---|---|---|
| `--canvas` | `#000000` | Page, panels, menus |
| `--surface` | `#0a0a0a` | Hover, the current row |
| `--surface-2` | `#161616` | Pressed, missing thumbnail |
| `--line` / `--line-strong` | white at 12% / 24% | Borders |
| `--text` / `--text-secondary` | `#ededed` / `#8f8f8f` | |
| `--accent` / `--accent-ink` | `#ffffff` / `#000000` | Primary button, progress, focus ring |
| `--danger` | `#ff6369` | Destructive actions and errors only |

Type: Geist (bundled), 400/500/600, sizes 12/14/16/20/28/48 px. Motion: 150/200/300 ms, `cubic-bezier(0.2, 0, 0, 1)`.

## Screens and their pattern

| Screen | Pattern | Primary action |
|---|---|---|
| Home | Overview | Resume |
| Series | Detail | Resume / Play the right episode |
| Import review | Form (editable table) | Add N files |
| Player | Full-bleed media | Play/pause |
| Command bar | Overlay | Jump |
| Empty library | Empty state | Add folder |
