# Website Redesign — Design Spec

**Date:** 2026-05-17
**Goal:** Full visual reset of `site/`. New theme, fonts, logo, favicon. Strict modern technical aesthetic.

## Decisions captured during brainstorming

| Decision | Choice | Why |
|---|---|---|
| Theme direction | Editorial Technical | Light background, generous whitespace, mono code blocks. Reads as serious docs, not a sales page. |
| Logo mark | Pixel grid (4×4 with 9 filled cells) | Abstract data-buffer / QR fragment. Distinctive, no vendor cliché. Legible at 16 px without simplification. |
| Accent color | Monochrome (no accent) | Strictest of the strict. Single black + four neutrals. CTA = solid black. Removes the existing `#00d992` brand color entirely. |
| Typography | IBM Plex Sans + IBM Plex Mono | Engineering-doc warmth; slight humanism; same family for sans and mono. |
| Landing layout | Spec-sheet (dense, datasheet-style) | Ruled tables, monospace metadata captions, multi-column hero. Reads "this is a datasheet". |
| Favicon | Full 9-cell pixel-grid pattern | Identical to logo at every size. Maximum recognition. |

---

## Brand system

### Palette (CSS custom properties)

```
--rt-bg:        #fafafa   /* page surface */
--rt-bg-alt:    #f3f3f3   /* ruled rows, code-block bg-light */
--rt-rule:      #e5e5e5   /* hairline borders */
--rt-rule-dash: #d4d4d4   /* dashed table separators */
--rt-muted:     #737373   /* secondary text */
--rt-quiet:     #999999   /* labels, captions */
--rt-fg:        #0a0a0b   /* primary text, mark, CTA */
--rt-fg-invert: #fafafa   /* text on black */
```

No accent color. Links: underlined `--rt-fg`. Hover: `--rt-muted` underline + `--rt-fg` text. Visited: same as link.

Dark mode (deferred for a follow-up — not in this redesign). The system is built so that swapping the two foreground/background pairs flips it later.

### Type scale

All in `IBM Plex Sans` unless noted. Mono = `IBM Plex Mono`.

| Token | Use | Size | Weight | Line-height | Letter-spacing |
|---|---|---|---|---|---|
| `--rt-text-display` | hero h1, page titles | 32 px (mobile) / 40 px (≥768 px) | 600 | 1.15 | -0.015 em |
| `--rt-text-h2` | section headings | 22 px | 600 | 1.2 | -0.01 em |
| `--rt-text-h3` | subsections, card titles | 16 px | 600 | 1.3 | -0.005 em |
| `--rt-text-body` | prose | 14 px | 400 | 1.6 | 0 |
| `--rt-text-small` | captions, table cells | 12 px | 400 | 1.55 | 0 |
| `--rt-text-meta` (mono) | labels, kbd-style, spec rows | 11 px | 500 | 1.4 | 0.04 em |
| `--rt-text-tag` (mono) | section eyebrows | 10 px | 500 | 1.4 | 0.18 em (uppercase) |
| `--rt-text-code` (mono) | inline + block code | 12 px | 500 | 1.5 | 0 |

Fonts loaded via self-hosted woff2 in `site/assets/fonts/`. No Google Fonts CDN at runtime. Font weights shipped: Plex Sans 400/500/600/700, Plex Mono 400/500/600. Latin subset only (woff2-subset). Total font payload target: ≤ 180 KB.

### Spacing & rhythm

8-pixel base unit. Section vertical padding 32/48 px. Hero padding 48/64 px. Page max-width 1080 px. Reading-column max-width 680 px.

### Borders & shape

Sharp corners everywhere (`border-radius: 0`). Hairline borders 1 px solid `--rt-rule`. Dashed separators 1 px dashed `--rt-rule-dash`. No shadows. No gradients.

---

## Logo

Single SVG. Lives at `site/assets/logo.svg`.

```
viewBox 0 0 96 96
filled cells (each 16×16 at 20-pixel pitch starting at 8):
  (0,0) (1,0)               — top-left pair
  (0,1)       (2,1)         — left + right-of-center
  (0,2) (1,2)               — bottom-left pair
  (0,3)       (2,3) (3,3)   — bottom-right triple
```

All cells fill `currentColor` so a single `color: var(--rt-fg)` controls it. The 4×4 grid template stays the same; faded cells (`opacity: 0.15`) appear in the *enlarged* hero variant only — the favicon and inline header marks use the solid-cells-only version.

Variants:
- `site/assets/logo.svg` — solid mark (favicon + inline use)
- `site/assets/logo-hero.svg` — same mark with the 7 unfilled cells rendered at 0.15 opacity, for the hero-size 96 px presentation. Optional; can be inlined into `index.html` instead of a separate file.

---

## Favicon

Single SVG favicon `site/assets/favicon.svg` — identical to `logo.svg` but with `viewBox="0 0 96 96"` and explicit `fill="#0a0a0b"` so it renders correctly without CSS. Browsers downscale cleanly because the cells fall on whole pixels at 16 px (the cell pitch is 20 → 16 px renders to ~3.3 px/cell, still distinguishable).

Also produce:
- `site/assets/favicon.ico` — multi-resolution ICO (16/32/48 px) for legacy browsers. Built once and committed; rendered by an offline tool (we'll commit the binary, no runtime build step).
- `site/assets/apple-touch-icon.png` — 180×180 PNG, solid black square padding (8 px) + mark centered. Committed binary.

Update `site.webmanifest` to include the SVG favicon. Existing `apple-touch-icon.png` gets replaced.

---

## Page structure

### Landing (`site/index.html`) — Spec-sheet layout

Sections, top to bottom:

1. **Header bar.**
   - Logo + wordmark on the left.
   - Monospace nav on the right: `OVERVIEW · CLI · DOCS · GITHUB` (uppercase, mono, 10 px).
   - 1 px bottom rule.

2. **Hero, two-column.**
   - Left (1.4fr): mono eyebrow `RALPHTERM / v0.1.0 / MIT`, h1 (`Drop-in for ralphex. Real PTY, real review gates.`), lead paragraph.
   - Right (1fr): inset spec card with mono key-value rows (language, binaries, license, upstream, install). 1 px solid border, no fill.
   - 1 px bottom rule.

3. **Capabilities table.**
   - Mono eyebrow `— capabilities`.
   - Two-column ruled table: capability ⇢ status (`supported` / `pending`).
   - Rows separated by 1 px dashed `--rt-rule-dash` hairlines.
   - Pulls content directly from the matrix in `docs/ralphex-compat.md`.

4. **Drop-in proof.**
   - Mono eyebrow `— drop in`.
   - Side-by-side `ralphex …` vs `ralphterm …` mono code blocks with a `≡` between them. Caption explains the alias binary.

5. **What it replaces.**
   - Mono eyebrow `— what it replaces`.
   - 3 short prose blocks in a single column: "Prompt mode breaks", "PTY mode survives", "Runs stay auditable". No card chrome — just spaced paragraphs with dashed rules between.

6. **Invoke.**
   - Mono eyebrow `— invoke`.
   - One mono code block (black background, off-white text), one paragraph below explaining `--tasks-only`.

7. **Drop-in features grid.**
   - Mono eyebrow `— more`.
   - 2×3 grid of feature cells: Worktrees, Notifications, Docker, Multi-provider, Review retry + patience, Plan auto-move. Each cell: mono label + one-line description.

8. **CTAs.**
   - Two buttons stacked horizontally: `GitHub` (solid black), `Read the docs` (transparent + 1 px black border). Above: mono caption `— next`.

9. **Footer.**
   - 1 px top rule. Mono row: `MIT · github · docs · site source`. 11 px text.

### Docs shell (`site/docs/*.html`) — Two-column

- Same header as landing.
- Left: 240 px-wide sticky sidebar with mono section labels (uppercase, 10 px) + indented page links (sans, 12 px, hairline left rule for the active link).
- Right: 680 px reading column.
- Page title is `--rt-text-h2`, prose is `--rt-text-body`, code is `--rt-text-code` in mono blocks with 1 px border (no fill in light mode).

Sidebar groups:
- `OVERVIEW` (Architecture, Security)
- `CLI` (Reference, Compat table, Migrate from ralphex)
- `RUN` (Workflows, Providers, Docker, Notifications)
- `API` (HTTP API, Dashboard)

### Existing pages to restyle (not restructure)

All current docs pages keep their content. They get the new shell + type stack. URLs unchanged. Internal links unchanged.

---

## File changes

### Replaced

- `site/index.html` — full rewrite around the spec-sheet layout above.
- `site/assets/styles.css` — full rewrite. Drop all existing classes and rewrite from scratch around the design tokens. Keep file path so any external links work.
- `site/docs/index.html` — replace nav and content shell.
- `site/docs/api.html` `architecture.html` `security.html` `workflows.html` `milestone-one.html` — restyle to the new docs shell. Content preserved verbatim.
- `site/docs/ralphex-compat.html` `migrate-from-ralphex.html` `cli.html` `providers.html` `notifications.html` `docker.html` — same: restyle, keep content.
- `site/assets/social-preview.png` — keep file path; replace with new monochrome version (committed binary).
- `site/assets/apple-touch-icon.png` — replace with new monochrome version (committed binary).
- `site/assets/favicon.svg` — new SVG favicon (the pixel grid).
- `site/assets/favicon.ico` — new multi-res ICO (committed binary).
- `site/site.webmanifest` — update `icons[]` to reference the new SVG/PNG.

### Added

- `site/assets/logo.svg` — solid 9-cell mark.
- `site/assets/fonts/IBMPlexSans-{400,500,600,700}.woff2`
- `site/assets/fonts/IBMPlexMono-{400,500,600}.woff2`
- `site/assets/fonts.css` — `@font-face` rules pointing at the self-hosted files; `font-display: swap`.

### Deleted (unused after rewrite)

- Any classes/sections in the old `styles.css` not referenced by new HTML — confirmed by grep before deletion.

### Unchanged

- `site/CNAME`, `site/robots.txt` — content stays.
- `site/sitemap.xml` — URLs are unchanged; only the `<lastmod>` values bump to 2026-05-17.
- `site/assets/comparison.svg` — kept; restyled via inline color tokens if visible anywhere.

---

## Test plan

Extend `tests/site_copy.rs`. New assertions:

- `landing_links_to_self_hosted_fonts_only` — assert `site/index.html` does not reference `fonts.googleapis.com` or `fonts.gstatic.com`.
- `landing_uses_ibm_plex_via_font_face` — assert `site/assets/styles.css` contains `@font-face` rules naming `IBM Plex Sans` and `IBM Plex Mono`.
- `landing_brand_color_is_monochrome` — assert `site/assets/styles.css` does not contain the legacy brand color `#00d992`.
- `landing_logo_is_pixel_grid` — assert `site/assets/logo.svg` contains exactly 9 `<rect` elements with `fill` that resolves to dark.
- `favicon_svg_present_and_matches_logo` — assert `site/assets/favicon.svg` exists and contains 9 `<rect` elements.
- `landing_uses_spec_sheet_eyebrows` — assert `site/index.html` contains at least four `— capabilities`, `— drop in`, `— what it replaces`, `— invoke` eyebrow strings.
- `webmanifest_references_new_icons` — assert `site/site.webmanifest` lists `favicon.svg` in its `icons[]`.

All existing `tests/site_copy.rs` assertions must continue to pass — content is preserved, only chrome changes. Where the existing tests asserted on legacy markup (e.g. specific class names like `.bg-grid`, `.terminal-card`), update the assertions to target the new markup that conveys the same intent.

---

## Out of scope (deferred follow-ups)

- Dark mode toggle (system is built to support it, but no toggle UI ships in this slice).
- Asciinema/animated terminal capture in the hero.
- Building `favicon.ico` from a script (committed once, manually).
- Restructuring the docs information architecture (sidebar groups follow current file set).
- Any change to `dashboard/` (the in-app run dashboard) — separate visual system, separate task.

---

## Acceptance

The redesign is done when:

1. `site/index.html` renders the spec-sheet hero with monochrome IBM Plex type, pixel-grid logo, no `#00d992`, no Google Fonts CDN.
2. All `site/docs/*.html` pages share the new shell and pass the existing content assertions plus the new ones above.
3. `site/assets/favicon.svg` and `favicon.ico` render the 9-cell pattern correctly in browser tabs (manual visual check).
4. `cargo test --all` passes including the extended `site_copy` suite.
5. Page weight (HTML + CSS + woff2 + favicon) is ≤ 280 KB on the landing page.
