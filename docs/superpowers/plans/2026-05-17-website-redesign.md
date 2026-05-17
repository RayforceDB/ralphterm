# Website Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the existing `site/` chrome with a monochrome editorial-technical theme: IBM Plex Sans + Plex Mono (self-hosted), pixel-grid logo + matching favicon, spec-sheet landing layout. All page content preserved.

**Architecture:** One CSS rewrite around design tokens. One landing-page rewrite around a spec-sheet layout. One docs shell rewrite (header + sidebar + reading column) applied to every existing `site/docs/*.html` page without altering body content. Self-host font woff2 files. Replace logo / favicon / apple-touch / social-preview assets. Extend `tests/site_copy.rs` with new monochrome + font assertions; keep all existing content assertions green.

**Tech Stack:** Static HTML + CSS, vanilla — no build step, no JS framework. IBM Plex woff2 subsets downloaded once and committed. Rust integration tests (`tests/site_copy.rs`) for asserting design decisions in the shipped files.

---

## File Structure

| Path | Responsibility |
|---|---|
| `site/assets/logo.svg` | NEW — 9-cell pixel-grid mark, single-color, fills `currentColor` |
| `site/assets/logo-hero.svg` | NEW — same mark plus 7 faded cells at opacity 0.15, for 96 px hero use |
| `site/assets/favicon.svg` | NEW — same 9-cell mark with explicit `fill="#0a0a0b"` |
| `site/assets/favicon.ico` | REPLACED — multi-res ICO (16/32/48 px) built offline once |
| `site/assets/apple-touch-icon.png` | REPLACED — 180×180 PNG, black square + centered mark |
| `site/assets/social-preview.png` | REPLACED — monochrome version, same dimensions |
| `site/assets/fonts/*.woff2` | NEW — Plex Sans 400/500/600/700 + Plex Mono 400/500/600 (latin subset) |
| `site/assets/fonts.css` | NEW — `@font-face` declarations, `font-display: swap` |
| `site/assets/styles.css` | REPLACED — full rewrite around design tokens; sharp corners; no shadows |
| `site/index.html` | REPLACED — spec-sheet layout (9 sections, see spec) |
| `site/site.webmanifest` | MODIFIED — `icons[]` references new SVG + PNG |
| `site/docs/index.html` | REPLACED — new shell, sidebar groups (OVERVIEW / CLI / RUN / API) |
| `site/docs/api.html` | RESTYLED — same content, new shell |
| `site/docs/architecture.html` | RESTYLED — same content, new shell |
| `site/docs/security.html` | RESTYLED — same content, new shell |
| `site/docs/workflows.html` | RESTYLED — same content, new shell |
| `site/docs/milestone-one.html` | RESTYLED — same content, new shell |
| `site/docs/cli.html` | RESTYLED — same content, new shell |
| `site/docs/ralphex-compat.html` | RESTYLED — same content, new shell |
| `site/docs/migrate-from-ralphex.html` | RESTYLED — same content, new shell |
| `site/docs/providers.html` | RESTYLED — same content, new shell |
| `site/docs/notifications.html` | RESTYLED — same content, new shell |
| `site/docs/docker.html` | RESTYLED — same content, new shell |
| `site/sitemap.xml` | MODIFIED — bump `<lastmod>` to 2026-05-17 |
| `tests/site_copy.rs` | MODIFIED — add 7 new assertions; update legacy-class assertions to new markup |

---

### Task 1: Download and commit IBM Plex woff2 files

**Goal:** Self-host the only fonts the site needs. No Google Fonts CDN at runtime.

**Files:**
- Create: `site/assets/fonts/IBMPlexSans-Regular.woff2` (weight 400)
- Create: `site/assets/fonts/IBMPlexSans-Medium.woff2` (weight 500)
- Create: `site/assets/fonts/IBMPlexSans-SemiBold.woff2` (weight 600)
- Create: `site/assets/fonts/IBMPlexSans-Bold.woff2` (weight 700)
- Create: `site/assets/fonts/IBMPlexMono-Regular.woff2` (weight 400)
- Create: `site/assets/fonts/IBMPlexMono-Medium.woff2` (weight 500)
- Create: `site/assets/fonts/IBMPlexMono-SemiBold.woff2` (weight 600)

- [ ] **Step 1: Create the fonts directory**

```bash
mkdir -p site/assets/fonts
```

- [ ] **Step 2: Download from the official IBM Plex GitHub releases**

The upstream-blessed source is the [IBM/plex](https://github.com/IBM/plex) repo, which publishes a per-family woff2 distribution. Use the [google-webfonts-helper](https://gwfh.mranftl.com/fonts) latin subset for compactness (each file ~20 KB):

```bash
cd site/assets/fonts
curl -fsSL -o IBMPlexSans-Regular.woff2  https://gwfh.mranftl.com/api/fonts/ibm-plex-sans/files/ibm-plex-sans-latin-400-normal.woff2
curl -fsSL -o IBMPlexSans-Medium.woff2   https://gwfh.mranftl.com/api/fonts/ibm-plex-sans/files/ibm-plex-sans-latin-500-normal.woff2
curl -fsSL -o IBMPlexSans-SemiBold.woff2 https://gwfh.mranftl.com/api/fonts/ibm-plex-sans/files/ibm-plex-sans-latin-600-normal.woff2
curl -fsSL -o IBMPlexSans-Bold.woff2     https://gwfh.mranftl.com/api/fonts/ibm-plex-sans/files/ibm-plex-sans-latin-700-normal.woff2
curl -fsSL -o IBMPlexMono-Regular.woff2  https://gwfh.mranftl.com/api/fonts/ibm-plex-mono/files/ibm-plex-mono-latin-400-normal.woff2
curl -fsSL -o IBMPlexMono-Medium.woff2   https://gwfh.mranftl.com/api/fonts/ibm-plex-mono/files/ibm-plex-mono-latin-500-normal.woff2
curl -fsSL -o IBMPlexMono-SemiBold.woff2 https://gwfh.mranftl.com/api/fonts/ibm-plex-mono/files/ibm-plex-mono-latin-600-normal.woff2
```

If `gwfh.mranftl.com` is unreachable, fall back to fontsource:
```
https://cdn.jsdelivr.net/fontsource/fonts/ibm-plex-sans@latest/latin-400-normal.woff2
```
(substitute `ibm-plex-mono`, weight, etc. as needed)

Expected: 7 woff2 files, each between 12 KB and 30 KB. Total ≤ 180 KB.

- [ ] **Step 3: Verify file integrity**

```bash
ls -la site/assets/fonts/
file site/assets/fonts/*.woff2 | head -3
```

Expected: every line says `Web Open Font Format (Version 2)`.

- [ ] **Step 4: Commit**

```bash
git add site/assets/fonts/
git commit -m "feat(site): self-host IBM Plex Sans + Mono woff2 subsets"
```

---

### Task 2: Author the pixel-grid logo SVG

**Goal:** Single-source logo, recolorable via `currentColor`, recognizable at any size.

**Files:**
- Create: `site/assets/logo.svg`
- Create: `site/assets/logo-hero.svg`

- [ ] **Step 1: Write `site/assets/logo.svg`**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96" role="img" aria-label="RalphTerm">
  <rect x="8"  y="8"  width="16" height="16" fill="currentColor"/>
  <rect x="28" y="8"  width="16" height="16" fill="currentColor"/>
  <rect x="8"  y="28" width="16" height="16" fill="currentColor"/>
  <rect x="48" y="28" width="16" height="16" fill="currentColor"/>
  <rect x="8"  y="48" width="16" height="16" fill="currentColor"/>
  <rect x="28" y="48" width="16" height="16" fill="currentColor"/>
  <rect x="8"  y="68" width="16" height="16" fill="currentColor"/>
  <rect x="48" y="68" width="16" height="16" fill="currentColor"/>
  <rect x="68" y="68" width="16" height="16" fill="currentColor"/>
</svg>
```

- [ ] **Step 2: Write `site/assets/logo-hero.svg`** (same 9 cells plus 7 faded cells)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96" role="img" aria-label="RalphTerm">
  <g fill="currentColor">
    <rect x="8"  y="8"  width="16" height="16"/>
    <rect x="28" y="8"  width="16" height="16"/>
    <rect x="8"  y="28" width="16" height="16"/>
    <rect x="48" y="28" width="16" height="16"/>
    <rect x="8"  y="48" width="16" height="16"/>
    <rect x="28" y="48" width="16" height="16"/>
    <rect x="8"  y="68" width="16" height="16"/>
    <rect x="48" y="68" width="16" height="16"/>
    <rect x="68" y="68" width="16" height="16"/>
  </g>
  <g fill="currentColor" opacity="0.15">
    <rect x="48" y="8"  width="16" height="16"/>
    <rect x="68" y="8"  width="16" height="16"/>
    <rect x="28" y="28" width="16" height="16"/>
    <rect x="68" y="28" width="16" height="16"/>
    <rect x="48" y="48" width="16" height="16"/>
    <rect x="68" y="48" width="16" height="16"/>
    <rect x="28" y="68" width="16" height="16"/>
  </g>
</svg>
```

- [ ] **Step 3: Verify both render**

```bash
xmllint --noout site/assets/logo.svg site/assets/logo-hero.svg
```

Expected: no output (valid XML, no errors).

- [ ] **Step 4: Commit**

```bash
git add site/assets/logo.svg site/assets/logo-hero.svg
git commit -m "feat(site): add pixel-grid logo SVGs"
```

---

### Task 3: Author the favicon SVG

**Goal:** A SVG favicon that renders the 9-cell pattern with explicit color (no CSS dependency).

**Files:**
- Create: `site/assets/favicon.svg` (replacing the existing one)

- [ ] **Step 1: Remove the legacy favicon SVG and write the new one**

```bash
rm site/assets/favicon.svg
```

Write `site/assets/favicon.svg`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96">
  <rect width="96" height="96" fill="#fafafa"/>
  <rect x="8"  y="8"  width="16" height="16" fill="#0a0a0b"/>
  <rect x="28" y="8"  width="16" height="16" fill="#0a0a0b"/>
  <rect x="8"  y="28" width="16" height="16" fill="#0a0a0b"/>
  <rect x="48" y="28" width="16" height="16" fill="#0a0a0b"/>
  <rect x="8"  y="48" width="16" height="16" fill="#0a0a0b"/>
  <rect x="28" y="48" width="16" height="16" fill="#0a0a0b"/>
  <rect x="8"  y="68" width="16" height="16" fill="#0a0a0b"/>
  <rect x="48" y="68" width="16" height="16" fill="#0a0a0b"/>
  <rect x="68" y="68" width="16" height="16" fill="#0a0a0b"/>
</svg>
```

- [ ] **Step 2: Verify it parses**

```bash
xmllint --noout site/assets/favicon.svg
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add site/assets/favicon.svg
git commit -m "feat(site): new monochrome pixel-grid favicon"
```

---

### Task 4: Generate the multi-resolution favicon.ico

**Goal:** Legacy-browser ICO file containing 16/32/48 px renderings of the same mark.

**Files:**
- Replace: `site/assets/favicon.ico`

- [ ] **Step 1: Render PNGs from the SVG**

If `librsvg2-bin` is available:

```bash
cd site/assets
for s in 16 32 48; do
  rsvg-convert -w $s -h $s favicon.svg -o /tmp/rt-favicon-$s.png
done
```

If `librsvg2-bin` is not installed but ImageMagick is:

```bash
for s in 16 32 48; do
  magick -background "#fafafa" -resize ${s}x${s} site/assets/favicon.svg /tmp/rt-favicon-$s.png
done
```

If neither is available, install librsvg first: `sudo apt-get install -y librsvg2-bin` (Debian/Ubuntu) or `brew install librsvg` (macOS).

- [ ] **Step 2: Bundle into a single multi-resolution ICO**

```bash
magick /tmp/rt-favicon-16.png /tmp/rt-favicon-32.png /tmp/rt-favicon-48.png site/assets/favicon.ico
```

If ImageMagick is missing but `icotool` (from `icoutils`) is available:

```bash
icotool -c -o site/assets/favicon.ico /tmp/rt-favicon-16.png /tmp/rt-favicon-32.png /tmp/rt-favicon-48.png
```

- [ ] **Step 3: Verify the ICO contains three resolutions**

```bash
file site/assets/favicon.ico
identify site/assets/favicon.ico   # if ImageMagick is available
```

Expected: `MS Windows icon resource - 3 icons`.

- [ ] **Step 4: Clean up the temporary PNGs and commit**

```bash
rm -f /tmp/rt-favicon-16.png /tmp/rt-favicon-32.png /tmp/rt-favicon-48.png
git add site/assets/favicon.ico
git commit -m "feat(site): regenerate favicon.ico from pixel-grid mark"
```

---

### Task 5: Regenerate apple-touch-icon.png and social-preview.png

**Goal:** Replace the two committed PNG brand assets with monochrome versions matching the new mark.

**Files:**
- Replace: `site/assets/apple-touch-icon.png` (180×180)
- Replace: `site/assets/social-preview.png` (preserve existing dimensions; check first with `identify`)

- [ ] **Step 1: Inspect the existing PNG dimensions**

```bash
identify site/assets/apple-touch-icon.png site/assets/social-preview.png
```

Note both dimension pairs (Apple's is fixed at 180×180; OG is typically 1200×630).

- [ ] **Step 2: Render the apple-touch-icon**

Write a one-off SVG that pads the mark inside a 180×180 black square at `/tmp/rt-apple.svg`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 180 180">
  <rect width="180" height="180" fill="#0a0a0b"/>
  <g transform="translate(42 42) scale(1)" fill="#fafafa">
    <rect x="8"  y="8"  width="16" height="16"/>
    <rect x="28" y="8"  width="16" height="16"/>
    <rect x="8"  y="28" width="16" height="16"/>
    <rect x="48" y="28" width="16" height="16"/>
    <rect x="8"  y="48" width="16" height="16"/>
    <rect x="28" y="48" width="16" height="16"/>
    <rect x="8"  y="68" width="16" height="16"/>
    <rect x="48" y="68" width="16" height="16"/>
    <rect x="68" y="68" width="16" height="16"/>
  </g>
</svg>
```

Render:

```bash
rsvg-convert -w 180 -h 180 /tmp/rt-apple.svg -o site/assets/apple-touch-icon.png
```

- [ ] **Step 3: Render the social preview at the previously identified dimensions**

Assume 1200×630. Write `/tmp/rt-social.svg`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 630">
  <rect width="1200" height="630" fill="#fafafa"/>
  <g transform="translate(96 96) scale(2)" fill="#0a0a0b">
    <rect x="8"  y="8"  width="16" height="16"/>
    <rect x="28" y="8"  width="16" height="16"/>
    <rect x="8"  y="28" width="16" height="16"/>
    <rect x="48" y="28" width="16" height="16"/>
    <rect x="8"  y="48" width="16" height="16"/>
    <rect x="28" y="48" width="16" height="16"/>
    <rect x="8"  y="68" width="16" height="16"/>
    <rect x="48" y="68" width="16" height="16"/>
    <rect x="68" y="68" width="16" height="16"/>
  </g>
  <text x="96" y="380" font-family="IBM Plex Sans, sans-serif" font-size="64" font-weight="600" fill="#0a0a0b" letter-spacing="-1">RalphTerm</text>
  <text x="96" y="450" font-family="IBM Plex Sans, sans-serif" font-size="34" font-weight="500" fill="#0a0a0b">Drop-in for ralphex. Real PTY, real review gates.</text>
  <text x="96" y="560" font-family="IBM Plex Mono, monospace" font-size="22" font-weight="500" fill="#737373">ralphterm --tasks-only docs/plans/feature.md</text>
</svg>
```

Render:

```bash
rsvg-convert -w 1200 -h 630 /tmp/rt-social.svg -o site/assets/social-preview.png
```

If the previous social preview was a different size (from Step 1), use that size in both the viewBox-aspect-aware `-w/-h` flags and adjust the SVG accordingly.

- [ ] **Step 4: Verify outputs**

```bash
identify site/assets/apple-touch-icon.png site/assets/social-preview.png
```

Expected: dimensions match the originals (180×180 and 1200×630 by default).

- [ ] **Step 5: Commit**

```bash
git add site/assets/apple-touch-icon.png site/assets/social-preview.png
rm -f /tmp/rt-apple.svg /tmp/rt-social.svg
git commit -m "feat(site): regenerate apple-touch-icon and social preview"
```

---

### Task 6: Write the new fonts.css

**Goal:** Single CSS file declaring `@font-face` for all seven weights, with `font-display: swap` so text appears even before woff2 loads.

**Files:**
- Create: `site/assets/fonts.css`

- [ ] **Step 1: Write `site/assets/fonts.css`**

```css
@font-face {
  font-family: 'IBM Plex Sans';
  src: url('/assets/fonts/IBMPlexSans-Regular.woff2') format('woff2');
  font-weight: 400;
  font-style: normal;
  font-display: swap;
}
@font-face {
  font-family: 'IBM Plex Sans';
  src: url('/assets/fonts/IBMPlexSans-Medium.woff2') format('woff2');
  font-weight: 500;
  font-style: normal;
  font-display: swap;
}
@font-face {
  font-family: 'IBM Plex Sans';
  src: url('/assets/fonts/IBMPlexSans-SemiBold.woff2') format('woff2');
  font-weight: 600;
  font-style: normal;
  font-display: swap;
}
@font-face {
  font-family: 'IBM Plex Sans';
  src: url('/assets/fonts/IBMPlexSans-Bold.woff2') format('woff2');
  font-weight: 700;
  font-style: normal;
  font-display: swap;
}
@font-face {
  font-family: 'IBM Plex Mono';
  src: url('/assets/fonts/IBMPlexMono-Regular.woff2') format('woff2');
  font-weight: 400;
  font-style: normal;
  font-display: swap;
}
@font-face {
  font-family: 'IBM Plex Mono';
  src: url('/assets/fonts/IBMPlexMono-Medium.woff2') format('woff2');
  font-weight: 500;
  font-style: normal;
  font-display: swap;
}
@font-face {
  font-family: 'IBM Plex Mono';
  src: url('/assets/fonts/IBMPlexMono-SemiBold.woff2') format('woff2');
  font-weight: 600;
  font-style: normal;
  font-display: swap;
}
```

- [ ] **Step 2: Commit**

```bash
git add site/assets/fonts.css
git commit -m "feat(site): add @font-face declarations for self-hosted Plex"
```

---

### Task 7: Replace `site/assets/styles.css` with the design-token rewrite

**Goal:** Single CSS file implementing the design system: tokens, base reset, header, hero, spec tables, code blocks, CTAs, docs shell.

**Files:**
- Replace: `site/assets/styles.css` (the entire file — delete current content)

- [ ] **Step 1: Write the new `site/assets/styles.css`**

```css
@import url('/assets/fonts.css');

:root {
  --rt-bg:        #fafafa;
  --rt-bg-alt:    #f3f3f3;
  --rt-rule:      #e5e5e5;
  --rt-rule-dash: #d4d4d4;
  --rt-muted:     #737373;
  --rt-quiet:     #999999;
  --rt-fg:        #0a0a0b;
  --rt-fg-invert: #fafafa;

  --rt-page-max:    1080px;
  --rt-read-max:    680px;
  --rt-pad-x:       24px;
  --rt-pad-x-lg:    48px;

  --rt-font-sans: 'IBM Plex Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  --rt-font-mono: 'IBM Plex Mono', ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
}

* { box-sizing: border-box; }
html, body {
  margin: 0;
  padding: 0;
  background: var(--rt-bg);
  color: var(--rt-fg);
  font-family: var(--rt-font-sans);
  font-size: 14px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}
a { color: var(--rt-fg); text-decoration: underline; text-underline-offset: 2px; }
a:hover { color: var(--rt-fg); text-decoration-color: var(--rt-muted); }
code, pre { font-family: var(--rt-font-mono); font-size: 12px; }
pre { margin: 0; }
hr { border: 0; border-top: 1px solid var(--rt-rule); margin: 0; }

/* Header */
.rt-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px var(--rt-pad-x);
  border-bottom: 1px solid var(--rt-rule);
  max-width: var(--rt-page-max);
  margin: 0 auto;
}
.rt-brand {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  color: var(--rt-fg);
  text-decoration: none;
  font-weight: 600;
}
.rt-brand svg { width: 22px; height: 22px; color: var(--rt-fg); }
.rt-brand-name { font-size: 14px; letter-spacing: -0.01em; }
.rt-nav {
  display: flex;
  gap: 18px;
  font-family: var(--rt-font-mono);
  font-size: 10px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--rt-muted);
}
.rt-nav a { color: var(--rt-muted); text-decoration: none; }
.rt-nav a:hover { color: var(--rt-fg); }

/* Layout */
.rt-page { max-width: var(--rt-page-max); margin: 0 auto; }
.rt-section { padding: 24px var(--rt-pad-x); border-bottom: 1px solid var(--rt-rule); }
.rt-section:last-of-type { border-bottom: none; }

/* Eyebrow */
.rt-eyebrow {
  font-family: var(--rt-font-mono);
  font-size: 10px;
  font-weight: 500;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--rt-quiet);
  margin: 0 0 14px;
}
.rt-eyebrow.is-tagline { letter-spacing: 0.15em; }

/* Headings */
h1, h2, h3, h4 { margin: 0; font-weight: 600; }
.rt-display {
  font-size: 32px;
  line-height: 1.15;
  letter-spacing: -0.015em;
  margin: 0 0 12px;
}
@media (min-width: 768px) {
  .rt-display { font-size: 40px; }
}
.rt-h2 { font-size: 22px; line-height: 1.2; letter-spacing: -0.01em; margin: 0 0 10px; }
.rt-h3 { font-size: 16px; line-height: 1.3; letter-spacing: -0.005em; margin: 0 0 6px; }
.rt-lead { font-size: 14px; line-height: 1.6; color: var(--rt-muted); margin: 0; }

/* Hero: two-column spec sheet */
.rt-hero {
  display: grid;
  grid-template-columns: 1fr;
  gap: 24px;
  padding: 36px var(--rt-pad-x) 32px;
  border-bottom: 1px solid var(--rt-rule);
}
@media (min-width: 768px) {
  .rt-hero { grid-template-columns: 1.4fr 1fr; gap: 40px; padding: 56px var(--rt-pad-x-lg) 48px; }
}
.rt-hero-spec {
  font-family: var(--rt-font-mono);
  font-size: 11px;
  border: 1px solid var(--rt-rule);
  padding: 14px 16px;
  background: var(--rt-bg);
}
.rt-hero-spec dt {
  display: inline-block;
  width: 7.5em;
  color: var(--rt-muted);
}
.rt-hero-spec dd { display: inline; margin: 0; }
.rt-hero-spec div + div { margin-top: 4px; }
.rt-hero-spec .rt-eyebrow { margin-bottom: 8px; }

/* Capability table */
.rt-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.rt-table th, .rt-table td { padding: 7px 0; text-align: left; }
.rt-table th { font-family: var(--rt-font-mono); font-weight: 500; font-size: 10px; letter-spacing: 0.1em; text-transform: uppercase; color: var(--rt-quiet); border-bottom: 1px solid var(--rt-rule); }
.rt-table td { border-top: 1px dashed var(--rt-rule-dash); }
.rt-table tbody tr:last-child td { border-bottom: 1px dashed var(--rt-rule-dash); }
.rt-table .rt-status { text-align: right; font-family: var(--rt-font-mono); font-size: 11px; }
.rt-table .rt-status.is-pending { color: var(--rt-muted); }

/* Code block (dark) */
.rt-code-block {
  background: var(--rt-fg);
  color: var(--rt-fg-invert);
  font-family: var(--rt-font-mono);
  font-size: 12px;
  font-weight: 500;
  padding: 12px 14px;
  overflow-x: auto;
  white-space: pre;
}
.rt-code-block + .rt-code-block { margin-top: 6px; }

/* Drop-in proof */
.rt-compare {
  display: grid;
  grid-template-columns: 1fr;
  gap: 8px;
  align-items: center;
}
@media (min-width: 768px) {
  .rt-compare { grid-template-columns: 1fr auto 1fr; }
}
.rt-compare .rt-equiv {
  font-family: var(--rt-font-mono);
  color: var(--rt-quiet);
  text-align: center;
  font-size: 14px;
}

/* Feature grid */
.rt-features {
  display: grid;
  grid-template-columns: 1fr;
  gap: 0;
  border-top: 1px solid var(--rt-rule);
}
@media (min-width: 640px) { .rt-features { grid-template-columns: 1fr 1fr; } }
@media (min-width: 1024px) { .rt-features { grid-template-columns: 1fr 1fr 1fr; } }
.rt-features > article {
  padding: 16px 18px;
  border-bottom: 1px solid var(--rt-rule);
  border-right: 1px solid var(--rt-rule);
}
.rt-features > article:nth-child(3n) { border-right: none; }
@media (max-width: 1023px) {
  .rt-features > article:nth-child(3n) { border-right: 1px solid var(--rt-rule); }
  .rt-features > article:nth-child(2n) { border-right: none; }
}
@media (max-width: 639px) {
  .rt-features > article:nth-child(2n) { border-right: none; }
  .rt-features > article { border-right: none; }
}
.rt-features .rt-feature-label {
  font-family: var(--rt-font-mono);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--rt-fg);
  margin: 0 0 6px;
}
.rt-features p { margin: 0; font-size: 12px; color: var(--rt-muted); line-height: 1.55; }

/* CTAs */
.rt-cta-row { display: flex; gap: 10px; flex-wrap: wrap; margin-top: 12px; }
.rt-btn {
  display: inline-flex;
  align-items: center;
  font-family: var(--rt-font-sans);
  font-weight: 500;
  font-size: 13px;
  padding: 9px 16px;
  text-decoration: none;
  border: 1px solid var(--rt-fg);
  color: var(--rt-fg);
  background: transparent;
  line-height: 1;
}
.rt-btn:hover { background: var(--rt-fg); color: var(--rt-fg-invert); }
.rt-btn.is-primary { background: var(--rt-fg); color: var(--rt-fg-invert); }
.rt-btn.is-primary:hover { background: transparent; color: var(--rt-fg); }

/* Footer */
.rt-footer {
  max-width: var(--rt-page-max);
  margin: 0 auto;
  padding: 18px var(--rt-pad-x);
  border-top: 1px solid var(--rt-rule);
  font-family: var(--rt-font-mono);
  font-size: 11px;
  color: var(--rt-muted);
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
}
.rt-footer a { color: var(--rt-muted); text-decoration: none; }
.rt-footer a:hover { color: var(--rt-fg); text-decoration: underline; }

/* Docs shell */
.rt-docs {
  display: grid;
  grid-template-columns: 1fr;
  max-width: var(--rt-page-max);
  margin: 0 auto;
}
@media (min-width: 1024px) {
  .rt-docs { grid-template-columns: 240px 1fr; }
}
.rt-sidebar {
  border-right: 0;
  border-bottom: 1px solid var(--rt-rule);
  padding: 20px var(--rt-pad-x);
}
@media (min-width: 1024px) {
  .rt-sidebar { border-right: 1px solid var(--rt-rule); border-bottom: 0; position: sticky; top: 0; height: 100vh; overflow-y: auto; }
}
.rt-sidebar-group { margin-bottom: 18px; }
.rt-sidebar-group-label {
  font-family: var(--rt-font-mono);
  font-size: 10px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--rt-quiet);
  margin: 0 0 6px;
}
.rt-sidebar ul { list-style: none; margin: 0; padding: 0; }
.rt-sidebar li { font-size: 13px; }
.rt-sidebar li a {
  display: block;
  padding: 4px 0 4px 10px;
  text-decoration: none;
  color: var(--rt-muted);
  border-left: 1px solid var(--rt-rule);
}
.rt-sidebar li a:hover { color: var(--rt-fg); }
.rt-sidebar li a.is-active { color: var(--rt-fg); border-left-color: var(--rt-fg); font-weight: 500; }

.rt-doc {
  padding: 28px var(--rt-pad-x) 48px;
  max-width: var(--rt-read-max);
}
.rt-doc h1 { font-size: 28px; line-height: 1.15; letter-spacing: -0.015em; margin: 0 0 18px; }
.rt-doc h2 { font-size: 18px; line-height: 1.25; letter-spacing: -0.005em; margin: 28px 0 10px; }
.rt-doc h3 { font-size: 14px; margin: 22px 0 8px; }
.rt-doc p { margin: 0 0 12px; }
.rt-doc code { background: var(--rt-bg-alt); padding: 1px 5px; border: 1px solid var(--rt-rule); }
.rt-doc pre {
  background: var(--rt-fg);
  color: var(--rt-fg-invert);
  padding: 12px 14px;
  font-family: var(--rt-font-mono);
  font-size: 12px;
  overflow-x: auto;
  margin: 0 0 12px;
}
.rt-doc pre code { background: transparent; border: 0; padding: 0; color: inherit; }
.rt-doc ul, .rt-doc ol { margin: 0 0 12px; padding-left: 20px; }
.rt-doc li { margin: 0 0 4px; }
.rt-doc table { width: 100%; border-collapse: collapse; margin: 8px 0 16px; font-size: 13px; }
.rt-doc table th, .rt-doc table td { padding: 6px 8px; border-bottom: 1px solid var(--rt-rule); text-align: left; }
.rt-doc table th { font-family: var(--rt-font-mono); font-size: 10px; letter-spacing: 0.1em; text-transform: uppercase; color: var(--rt-quiet); border-bottom: 1px solid var(--rt-rule); }
.rt-doc blockquote { border-left: 1px solid var(--rt-rule); margin: 0 0 12px; padding-left: 12px; color: var(--rt-muted); }

/* Focus */
:focus-visible { outline: 2px solid var(--rt-fg); outline-offset: 2px; }
```

- [ ] **Step 2: Verify the file parses (no syntax errors)**

```bash
node -e "console.log('css length:', require('fs').readFileSync('site/assets/styles.css','utf8').length)"
```

Expected: a positive byte count, no exception. (We're not running a CSS validator — this is just a smoke check.)

- [ ] **Step 3: Commit**

```bash
git add site/assets/styles.css
git commit -m "feat(site): rewrite stylesheet around monochrome editorial-technical tokens"
```

---

### Task 8: Rewrite `site/index.html` as a spec-sheet landing page

**Goal:** Nine sections in the order specified in the design doc, using the new CSS classes.

**Files:**
- Replace: `site/index.html`

- [ ] **Step 1: Write the new `site/index.html`**

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>RalphTerm — Drop-in for ralphex. Real PTY. Real review gates.</title>
  <meta name="description" content="Drop-in replacement for ralphex. Runs your existing plans through the official AI CLIs in real PTY sessions. No --print, no -p, no one-shot prompt mode.">
  <meta name="keywords" content="RalphTerm, ralphex, Claude Code, Codex, AI coding agents, plan runner, terminal automation, CLI orchestration, autonomous coding, PTY">
  <meta name="robots" content="index, follow">
  <link rel="canonical" href="https://ralphterm.rayforcedb.com/">

  <meta property="og:type" content="website">
  <meta property="og:title" content="RalphTerm — Drop-in for ralphex. Real PTY.">
  <meta property="og:description" content="Drop-in replacement for ralphex. Runs your existing plans through the official AI CLIs in real PTY sessions.">
  <meta property="og:url" content="https://ralphterm.rayforcedb.com/">
  <meta property="og:image" content="https://ralphterm.rayforcedb.com/assets/social-preview.png">
  <meta property="og:site_name" content="RalphTerm">

  <meta name="twitter:card" content="summary_large_image">
  <meta name="twitter:title" content="RalphTerm — Drop-in for ralphex. Real PTY.">
  <meta name="twitter:description" content="Drop-in replacement for ralphex. Real PTY, real review gates.">
  <meta name="twitter:image" content="https://ralphterm.rayforcedb.com/assets/social-preview.png">

  <link rel="icon" href="/assets/favicon.svg" type="image/svg+xml">
  <link rel="alternate icon" href="/assets/favicon.ico" type="image/x-icon">
  <link rel="apple-touch-icon" href="/assets/apple-touch-icon.png">
  <link rel="manifest" href="/site.webmanifest">
  <link rel="stylesheet" href="/assets/styles.css">

  <script type="application/ld+json">
  {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    "name": "RalphTerm",
    "description": "Drop-in replacement for ralphex. Runs the official AI CLIs in real PTY sessions, validates task output, requires independent review when configured, commits progress, and keeps transcripts.",
    "url": "https://ralphterm.rayforcedb.com",
    "applicationCategory": "DeveloperApplication",
    "operatingSystem": "Linux, macOS",
    "codeRepository": "https://github.com/RayforceDB/ralphterm",
    "license": "https://opensource.org/licenses/MIT",
    "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"}
  }
  </script>
</head>
<body>
  <header class="rt-header">
    <a class="rt-brand" href="/" aria-label="RalphTerm home">
      <svg viewBox="0 0 96 96" aria-hidden="true">
        <rect x="8"  y="8"  width="16" height="16" fill="currentColor"/>
        <rect x="28" y="8"  width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="28" width="16" height="16" fill="currentColor"/>
        <rect x="48" y="28" width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="48" width="16" height="16" fill="currentColor"/>
        <rect x="28" y="48" width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="68" width="16" height="16" fill="currentColor"/>
        <rect x="48" y="68" width="16" height="16" fill="currentColor"/>
        <rect x="68" y="68" width="16" height="16" fill="currentColor"/>
      </svg>
      <span class="rt-brand-name">ralphterm</span>
    </a>
    <nav class="rt-nav" aria-label="Primary navigation">
      <a href="#overview">Overview</a>
      <a href="#cli">CLI</a>
      <a href="/docs/">Docs</a>
      <a href="https://github.com/RayforceDB/ralphterm">GitHub</a>
    </nav>
  </header>

  <main class="rt-page">
    <!-- Hero -->
    <section class="rt-hero" id="overview">
      <div>
        <p class="rt-eyebrow is-tagline">RALPHTERM / v0.1.0 / MIT</p>
        <h1 class="rt-display">Drop-in for ralphex. Real PTY, real review gates.</h1>
        <p class="rt-lead">Run your existing plans through the official AI CLIs in a real terminal. Same flags, same plan format, harder review gate. No <code>--print</code>, no <code>-p</code>, no one-shot prompt mode.</p>
        <div class="rt-cta-row">
          <a class="rt-btn is-primary" href="https://github.com/RayforceDB/ralphterm">GitHub</a>
          <a class="rt-btn" href="/docs/migrate-from-ralphex.html">Migrate from ralphex</a>
        </div>
      </div>
      <aside class="rt-hero-spec" aria-label="At a glance">
        <p class="rt-eyebrow">— spec</p>
        <div><dt>language</dt>  <dd>Rust 2021</dd></div>
        <div><dt>binaries</dt>  <dd>ralphterm, ralphex</dd></div>
        <div><dt>license</dt>   <dd>MIT</dd></div>
        <div><dt>upstream</dt>  <dd>ralphex / claude / codex</dd></div>
        <div><dt>install</dt>   <dd>cargo install --git&hellip;</dd></div>
      </aside>
    </section>

    <!-- Capabilities -->
    <section class="rt-section">
      <p class="rt-eyebrow">— capabilities</p>
      <table class="rt-table">
        <thead><tr><th>Surface</th><th class="rt-status">Status</th></tr></thead>
        <tbody>
          <tr><td>ralphex CLI surface</td><td class="rt-status">supported</td></tr>
          <tr><td>notifications &nbsp;·&nbsp; telegram, slack, email, webhook</td><td class="rt-status">supported</td></tr>
          <tr><td>docker isolation</td><td class="rt-status">supported</td></tr>
          <tr><td>providers &nbsp;·&nbsp; codex, copilot, gemini, opencode</td><td class="rt-status">supported</td></tr>
          <tr><td>worktrees &nbsp;·&nbsp; branch flag, plan-slug auto-id</td><td class="rt-status">supported</td></tr>
          <tr><td>review retry &nbsp;·&nbsp; with patience / stalemate detection</td><td class="rt-status">supported</td></tr>
          <tr><td>filesystem watch &nbsp;·&nbsp; idle timeout</td><td class="rt-status is-pending">pending</td></tr>
        </tbody>
      </table>
    </section>

    <!-- Drop-in proof -->
    <section class="rt-section" id="cli">
      <p class="rt-eyebrow">— drop in</p>
      <div class="rt-compare">
        <pre class="rt-code-block">$ ralphex --tasks-only docs/plans/feature.md</pre>
        <span class="rt-equiv" aria-hidden="true">≡</span>
        <pre class="rt-code-block">$ ralphterm --tasks-only docs/plans/feature.md</pre>
      </div>
      <p style="margin-top:12px;font-size:12px;color:var(--rt-muted)">Same flags, same plan, same output. The <code>ralphex</code> binary is an alias for <code>ralphterm</code> — point your scripts at either.</p>
    </section>

    <!-- What it replaces -->
    <section class="rt-section">
      <p class="rt-eyebrow">— what it replaces</p>
      <p style="max-width:var(--rt-read-max);margin-bottom:12px"><strong>Prompt mode breaks.</strong> Long plan runs hit login screens, approval prompts, rate limits, follow-up questions, and output changes.</p>
      <hr>
      <p style="max-width:var(--rt-read-max);margin:12px 0"><strong>PTY mode survives.</strong> The official CLI keeps its real terminal. RalphTerm watches output and sends input like a controlled operator.</p>
      <hr>
      <p style="max-width:var(--rt-read-max);margin-top:12px"><strong>Runs stay auditable.</strong> Every task exposes validation results, completion signals, commit hashes, and the raw transcript.</p>
    </section>

    <!-- Invoke -->
    <section class="rt-section">
      <p class="rt-eyebrow">— invoke</p>
      <pre class="rt-code-block">$ ralphterm --tasks-only docs/plans/feature.md</pre>
      <p style="margin-top:10px;font-size:12px;color:var(--rt-muted)">Tasks-only mode skips the independent review gate. For full mode add <code>--external-review-tool=custom --custom-review-script &lt;cmd&gt;</code>.</p>
    </section>

    <!-- Features grid -->
    <section class="rt-section" style="padding-bottom:0">
      <p class="rt-eyebrow">— more</p>
    </section>
    <div class="rt-features">
      <article><p class="rt-feature-label">Worktrees</p><p>Isolated git worktrees per plan; auto-derived branch slug from the plan filename.</p></article>
      <article><p class="rt-feature-label">Notifications</p><p>Telegram, Slack, Email (SMTP), and Webhook deliveries on plan / task / review / rate-limit events.</p></article>
      <article><p class="rt-feature-label">Docker</p><p>Optional containerized execution. Honors RALPHEX_EXTRA_VOLUMES + EXTRA_ENV.</p></article>
      <article><p class="rt-feature-label">Multi-provider</p><p>Bundled wrappers for Codex, Copilot, Gemini, and OpenCode CLIs.</p></article>
      <article><p class="rt-feature-label">Review retry</p><p>Implementer + reviewer fixer loop with patience / stalemate detection.</p></article>
      <article><p class="rt-feature-label">Plan auto-move</p><p>Successful plans relocate to docs/plans/completed/ on configurable opt-in.</p></article>
    </div>

    <!-- CTAs -->
    <section class="rt-section">
      <p class="rt-eyebrow">— next</p>
      <div class="rt-cta-row">
        <a class="rt-btn is-primary" href="https://github.com/RayforceDB/ralphterm">GitHub</a>
        <a class="rt-btn" href="/docs/">Read the docs</a>
        <a class="rt-btn" href="/docs/migrate-from-ralphex.html">Migrate from ralphex</a>
      </div>
    </section>
  </main>

  <footer class="rt-footer">
    <span>MIT License</span>
    <a href="https://github.com/RayforceDB/ralphterm">GitHub</a>
    <a href="/docs/">Docs</a>
    <a href="/docs/ralphex-compat.html">Ralphex compat</a>
  </footer>
</body>
</html>
```

- [ ] **Step 2: Open it in a browser locally (manual check)**

```bash
python3 -m http.server -d site 8765 &
sleep 1
xdg-open http://localhost:8765/ || open http://localhost:8765/
# After visual check, stop the server:
kill %1 2>/dev/null
```

Expected: page renders with monochrome theme, IBM Plex Sans loaded, pixel-grid logo at top-left, two-column hero. If fonts haven't been fetched yet you'll see system fallback briefly — that's by design (`font-display: swap`).

- [ ] **Step 3: Commit**

```bash
git add site/index.html
git commit -m "feat(site): rewrite landing as spec-sheet layout"
```

---

### Task 9: Restyle the docs index

**Goal:** Update `site/docs/index.html` to use the new shell + sidebar groups.

**Files:**
- Replace: `site/docs/index.html`

- [ ] **Step 1: Write the new docs index**

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>RalphTerm Documentation</title>
  <meta name="description" content="RalphTerm documentation index — architecture, CLI reference, ralphex compatibility, providers, docker, notifications, workflows.">
  <link rel="icon" href="/assets/favicon.svg" type="image/svg+xml">
  <link rel="alternate icon" href="/assets/favicon.ico" type="image/x-icon">
  <link rel="manifest" href="/site.webmanifest">
  <link rel="stylesheet" href="/assets/styles.css">
</head>
<body>
  <header class="rt-header">
    <a class="rt-brand" href="/" aria-label="RalphTerm home">
      <svg viewBox="0 0 96 96" aria-hidden="true">
        <rect x="8"  y="8"  width="16" height="16" fill="currentColor"/><rect x="28" y="8"  width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="28" width="16" height="16" fill="currentColor"/><rect x="48" y="28" width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="48" width="16" height="16" fill="currentColor"/><rect x="28" y="48" width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="68" width="16" height="16" fill="currentColor"/><rect x="48" y="68" width="16" height="16" fill="currentColor"/><rect x="68" y="68" width="16" height="16" fill="currentColor"/>
      </svg>
      <span class="rt-brand-name">ralphterm</span>
    </a>
    <nav class="rt-nav" aria-label="Primary navigation">
      <a href="/">Home</a>
      <a href="/docs/">Docs</a>
      <a href="https://github.com/RayforceDB/ralphterm">GitHub</a>
    </nav>
  </header>

  <div class="rt-docs">
    <aside class="rt-sidebar" aria-label="Docs navigation">
      <div class="rt-sidebar-group">
        <p class="rt-sidebar-group-label">Overview</p>
        <ul>
          <li><a href="/docs/architecture.html">Architecture</a></li>
          <li><a href="/docs/security.html">Security</a></li>
          <li><a href="/docs/milestone-one.html">Milestone 1</a></li>
        </ul>
      </div>
      <div class="rt-sidebar-group">
        <p class="rt-sidebar-group-label">CLI</p>
        <ul>
          <li><a href="/docs/cli.html">CLI reference</a></li>
          <li><a href="/docs/ralphex-compat.html">Ralphex compat</a></li>
          <li><a href="/docs/migrate-from-ralphex.html">Migrate from ralphex</a></li>
        </ul>
      </div>
      <div class="rt-sidebar-group">
        <p class="rt-sidebar-group-label">Run</p>
        <ul>
          <li><a href="/docs/workflows.html">Workflows</a></li>
          <li><a href="/docs/providers.html">Providers</a></li>
          <li><a href="/docs/docker.html">Docker</a></li>
          <li><a href="/docs/notifications.html">Notifications</a></li>
        </ul>
      </div>
      <div class="rt-sidebar-group">
        <p class="rt-sidebar-group-label">API</p>
        <ul>
          <li><a href="/docs/api.html" class="is-active">HTTP API</a></li>
        </ul>
      </div>
    </aside>

    <article class="rt-doc">
      <h1>Documentation</h1>
      <p class="rt-lead">RalphTerm is a drop-in replacement for ralphex with PTY-native execution. Start with <a href="/docs/migrate-from-ralphex.html">Migrate from ralphex</a> if you already have a ralphex setup; otherwise <a href="/docs/cli.html">CLI reference</a> covers every supported flag.</p>

      <h2>Overview</h2>
      <p><strong>Architecture.</strong> <a href="/docs/architecture.html">How the daemon, PTY runtime, runner, and event store fit together.</a></p>
      <p><strong>Security.</strong> <a href="/docs/security.html">Authentication, rate-limiting boundaries, isolation guarantees.</a></p>
      <p><strong>Milestone 1.</strong> <a href="/docs/milestone-one.html">Autonomous engineering workflow shipped on the PTY core.</a></p>

      <h2>CLI</h2>
      <p><strong>CLI reference.</strong> <a href="/docs/cli.html">Every flag from <code>--help</code>, grouped by purpose.</a></p>
      <p><strong>Ralphex compatibility.</strong> <a href="/docs/ralphex-compat.html">Per-flag status table: supported, accepted, pending.</a></p>
      <p><strong>Migration guide.</strong> <a href="/docs/migrate-from-ralphex.html">Step-by-step from an existing ralphex install.</a></p>

      <h2>Run</h2>
      <p><strong>Workflows.</strong> <a href="/docs/workflows.html">Plan run, review-only, external-only modes.</a></p>
      <p><strong>Providers.</strong> <a href="/docs/providers.html">Codex, Copilot, Gemini, OpenCode wrappers.</a></p>
      <p><strong>Docker.</strong> <a href="/docs/docker.html">Isolated container execution.</a></p>
      <p><strong>Notifications.</strong> <a href="/docs/notifications.html">Telegram / Slack / Email / Webhook configuration.</a></p>

      <h2>API</h2>
      <p><strong>HTTP API.</strong> <a href="/docs/api.html">Session and run endpoints, transcripts, events.</a></p>
    </article>
  </div>

  <footer class="rt-footer">
    <span>MIT License</span>
    <a href="https://github.com/RayforceDB/ralphterm">GitHub</a>
    <a href="/">Home</a>
  </footer>
</body>
</html>
```

- [ ] **Step 2: Commit**

```bash
git add site/docs/index.html
git commit -m "feat(site): rewrite docs index around new shell"
```

---

### Task 10: Restyle individual docs pages

**Goal:** Apply the new shell + classes to all twelve `site/docs/*.html` pages without altering their body content.

**Files (all are MODIFIED, content preserved):**
- `site/docs/api.html`
- `site/docs/architecture.html`
- `site/docs/security.html`
- `site/docs/workflows.html`
- `site/docs/milestone-one.html`
- `site/docs/cli.html`
- `site/docs/ralphex-compat.html`
- `site/docs/migrate-from-ralphex.html`
- `site/docs/providers.html`
- `site/docs/notifications.html`
- `site/docs/docker.html`

- [ ] **Step 1: For each file, replace the `<head>` + shell with the new template, preserving the inner body content verbatim**

The template:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{{PAGE_TITLE}} — RalphTerm</title>
  <meta name="description" content="{{PAGE_DESCRIPTION}}">
  <link rel="icon" href="/assets/favicon.svg" type="image/svg+xml">
  <link rel="alternate icon" href="/assets/favicon.ico" type="image/x-icon">
  <link rel="manifest" href="/site.webmanifest">
  <link rel="stylesheet" href="/assets/styles.css">
</head>
<body>
  <header class="rt-header">
    <a class="rt-brand" href="/" aria-label="RalphTerm home">
      <svg viewBox="0 0 96 96" aria-hidden="true">
        <rect x="8"  y="8"  width="16" height="16" fill="currentColor"/><rect x="28" y="8"  width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="28" width="16" height="16" fill="currentColor"/><rect x="48" y="28" width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="48" width="16" height="16" fill="currentColor"/><rect x="28" y="48" width="16" height="16" fill="currentColor"/>
        <rect x="8"  y="68" width="16" height="16" fill="currentColor"/><rect x="48" y="68" width="16" height="16" fill="currentColor"/><rect x="68" y="68" width="16" height="16" fill="currentColor"/>
      </svg>
      <span class="rt-brand-name">ralphterm</span>
    </a>
    <nav class="rt-nav" aria-label="Primary navigation">
      <a href="/">Home</a>
      <a href="/docs/">Docs</a>
      <a href="https://github.com/RayforceDB/ralphterm">GitHub</a>
    </nav>
  </header>

  <div class="rt-docs">
    <aside class="rt-sidebar" aria-label="Docs navigation">
      <!-- Same group blocks as in docs/index.html. -->
      <!-- Mark the active page's <a> with class="is-active". -->
      <!-- Identical sidebar markup; duplicated for now — no template engine. -->
    </aside>
    <article class="rt-doc">
      {{ORIGINAL_BODY_CONTENT}}
    </article>
  </div>

  <footer class="rt-footer">
    <span>MIT License</span>
    <a href="https://github.com/RayforceDB/ralphterm">GitHub</a>
    <a href="/">Home</a>
  </footer>
</body>
</html>
```

For each page:

1. Open the existing file.
2. Capture the visible body content (everything between the existing `<main>` / page-content open and close tags — strip any old header/nav/footer/sidebar wrappers but keep paragraphs/lists/code/tables intact).
3. Replace the entire file with the template above.
4. Paste the captured content into `{{ORIGINAL_BODY_CONTENT}}`.
5. Fill in `{{PAGE_TITLE}}` and `{{PAGE_DESCRIPTION}}` per page (use the existing `<title>` and `<meta name="description">` values).
6. Inside the sidebar, set `class="is-active"` on the `<a>` that points to the current page.

The exact sidebar block to paste verbatim into every page (then add the `is-active` class to one link per page):

```html
<div class="rt-sidebar-group">
  <p class="rt-sidebar-group-label">Overview</p>
  <ul>
    <li><a href="/docs/architecture.html">Architecture</a></li>
    <li><a href="/docs/security.html">Security</a></li>
    <li><a href="/docs/milestone-one.html">Milestone 1</a></li>
  </ul>
</div>
<div class="rt-sidebar-group">
  <p class="rt-sidebar-group-label">CLI</p>
  <ul>
    <li><a href="/docs/cli.html">CLI reference</a></li>
    <li><a href="/docs/ralphex-compat.html">Ralphex compat</a></li>
    <li><a href="/docs/migrate-from-ralphex.html">Migrate from ralphex</a></li>
  </ul>
</div>
<div class="rt-sidebar-group">
  <p class="rt-sidebar-group-label">Run</p>
  <ul>
    <li><a href="/docs/workflows.html">Workflows</a></li>
    <li><a href="/docs/providers.html">Providers</a></li>
    <li><a href="/docs/docker.html">Docker</a></li>
    <li><a href="/docs/notifications.html">Notifications</a></li>
  </ul>
</div>
<div class="rt-sidebar-group">
  <p class="rt-sidebar-group-label">API</p>
  <ul>
    <li><a href="/docs/api.html">HTTP API</a></li>
  </ul>
</div>
```

- [ ] **Step 2: Verify content didn't regress for one sample page**

```bash
diff <(grep -E '^[A-Za-z]|^- |^[0-9]|^## ' /tmp/api-html-before.txt 2>/dev/null || echo) <(grep -E '^[A-Za-z]|^- |^[0-9]|^## ' site/docs/api.html)
```

(More robust: just visually open one page in the local server and confirm headings and code blocks render.)

- [ ] **Step 3: Run `cargo test --test site_copy` to confirm none of the existing content assertions broke**

```bash
cargo test --test site_copy 2>&1 | tail -10
```

Expected: all currently-passing tests still pass. If any fail because they targeted a legacy class name (`.bg-grid`, `.terminal-card`, `.hero-actions`, `.cards.three`), update those assertions to target the new equivalent (`.rt-section`, `.rt-code-block`, `.rt-cta-row`, `.rt-features`). Do not change what the assertion is verifying — only the selector / phrase used to verify it.

- [ ] **Step 4: Commit**

```bash
git add site/docs/
git commit -m "feat(site): apply new docs shell to all pages"
```

---

### Task 11: Update `site/site.webmanifest`

**Goal:** Point at the new SVG favicon + replaced apple-touch-icon.

**Files:**
- Modify: `site/site.webmanifest`

- [ ] **Step 1: Read the current manifest**

```bash
cat site/site.webmanifest
```

- [ ] **Step 2: Replace with the new manifest**

```json
{
  "name": "RalphTerm",
  "short_name": "RalphTerm",
  "description": "Drop-in replacement for ralphex with PTY-native execution.",
  "icons": [
    { "src": "/assets/favicon.svg", "type": "image/svg+xml", "sizes": "any" },
    { "src": "/assets/apple-touch-icon.png", "type": "image/png", "sizes": "180x180" }
  ],
  "theme_color": "#fafafa",
  "background_color": "#fafafa",
  "display": "browser",
  "start_url": "/"
}
```

- [ ] **Step 3: Commit**

```bash
git add site/site.webmanifest
git commit -m "feat(site): point webmanifest at new icons + monochrome theme"
```

---

### Task 12: Bump sitemap.xml lastmod values

**Goal:** Mark every site URL as updated today so search engines re-crawl after the redesign.

**Files:**
- Modify: `site/sitemap.xml`

- [ ] **Step 1: Update `<lastmod>` on every URL**

Replace each `<lastmod>...</lastmod>` value inside `site/sitemap.xml` with `<lastmod>2026-05-17</lastmod>`. Do not add or remove URLs.

```bash
sed -i 's|<lastmod>[^<]*</lastmod>|<lastmod>2026-05-17</lastmod>|g' site/sitemap.xml
```

- [ ] **Step 2: Confirm**

```bash
grep -c '<lastmod>2026-05-17</lastmod>' site/sitemap.xml
```

Expected: a count equal to the number of `<url>` blocks in the file.

- [ ] **Step 3: Commit**

```bash
git add site/sitemap.xml
git commit -m "docs(site): bump sitemap lastmod after redesign"
```

---

### Task 13: Extend `tests/site_copy.rs` with the new design assertions

**Goal:** Add the seven new assertions from the design spec.

**Files:**
- Modify: `tests/site_copy.rs`

- [ ] **Step 1: Append new tests at the end of the file**

```rust
#[test]
fn landing_links_to_self_hosted_fonts_only() {
    let html = std::fs::read_to_string("site/index.html").expect("read site/index.html");
    assert!(
        !html.contains("fonts.googleapis.com") && !html.contains("fonts.gstatic.com"),
        "landing page must not reference Google Fonts CDN at runtime"
    );
}

#[test]
fn stylesheet_uses_ibm_plex_via_font_face() {
    let css = std::fs::read_to_string("site/assets/styles.css").expect("read styles.css");
    assert!(
        css.contains("IBM Plex Sans") && css.contains("IBM Plex Mono"),
        "styles.css must reference both IBM Plex Sans and IBM Plex Mono"
    );
    let fonts_css = std::fs::read_to_string("site/assets/fonts.css").expect("read fonts.css");
    assert!(
        fonts_css.matches("@font-face").count() >= 7,
        "fonts.css must declare at least seven @font-face rules"
    );
}

#[test]
fn landing_brand_color_is_monochrome() {
    let css = std::fs::read_to_string("site/assets/styles.css").expect("read styles.css");
    assert!(
        !css.contains("#00d992"),
        "styles.css must not contain the legacy brand color #00d992"
    );
    let landing = std::fs::read_to_string("site/index.html").expect("read site/index.html");
    assert!(
        !landing.contains("#00d992"),
        "site/index.html must not contain the legacy brand color #00d992"
    );
}

#[test]
fn landing_logo_is_pixel_grid() {
    let svg = std::fs::read_to_string("site/assets/logo.svg").expect("read logo.svg");
    let rect_count = svg.matches("<rect").count();
    assert_eq!(rect_count, 9, "logo.svg must contain exactly 9 <rect> elements (got {rect_count})");
}

#[test]
fn favicon_svg_present_and_matches_logo_pattern() {
    let svg = std::fs::read_to_string("site/assets/favicon.svg").expect("read favicon.svg");
    let dark_rect_count = svg.matches("fill=\"#0a0a0b\"").count();
    assert!(
        dark_rect_count >= 9,
        "favicon.svg must render the 9-cell pixel grid in dark color (counted {dark_rect_count})"
    );
}

#[test]
fn landing_uses_spec_sheet_eyebrows() {
    let html = std::fs::read_to_string("site/index.html").expect("read site/index.html");
    for marker in &["— capabilities", "— drop in", "— what it replaces", "— invoke"] {
        assert!(
            html.contains(marker),
            "landing must include the '{marker}' eyebrow"
        );
    }
}

#[test]
fn webmanifest_references_new_icons() {
    let manifest = std::fs::read_to_string("site/site.webmanifest").expect("read webmanifest");
    assert!(
        manifest.contains("\"src\": \"/assets/favicon.svg\""),
        "webmanifest must reference favicon.svg in its icons[]"
    );
    assert!(
        manifest.contains("\"src\": \"/assets/apple-touch-icon.png\""),
        "webmanifest must reference apple-touch-icon.png in its icons[]"
    );
}
```

- [ ] **Step 2: Run only the new tests**

```bash
cargo test --test site_copy landing_links_to_self_hosted_fonts_only stylesheet_uses_ibm_plex_via_font_face landing_brand_color_is_monochrome landing_logo_is_pixel_grid favicon_svg_present_and_matches_logo_pattern landing_uses_spec_sheet_eyebrows webmanifest_references_new_icons 2>&1 | tail -15
```

Expected: 7 passed.

- [ ] **Step 3: Run the full file to confirm nothing else broke**

```bash
cargo test --test site_copy 2>&1 | tail -5
```

Expected: all tests pass. If pre-existing tests now fail, fix them by updating their selectors to the new markup (per Task 10 Step 3 guidance).

- [ ] **Step 4: Run the entire workspace test suite**

```bash
cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all 2>&1 | tail -5
```

Expected: clean, all green.

- [ ] **Step 5: Commit**

```bash
git add tests/site_copy.rs
git commit -m "test(site): assert monochrome theme, self-hosted fonts, pixel-grid logo"
```

---

### Task 14: Verify final page-weight budget

**Goal:** Confirm landing page assets total ≤ 280 KB.

**Files:**
- None modified.

- [ ] **Step 1: Measure**

```bash
sizeof() { wc -c < "$1" | tr -d ' '; }
INDEX=$(sizeof site/index.html)
CSS=$(sizeof site/assets/styles.css)
FONTS_CSS=$(sizeof site/assets/fonts.css)
FONTS=$(find site/assets/fonts -name '*.woff2' -printf '%s\n' | paste -sd+ | bc)
FAV_SVG=$(sizeof site/assets/favicon.svg)
LOGO_SVG=$(sizeof site/assets/logo.svg)
TOTAL=$((INDEX + CSS + FONTS_CSS + FONTS + FAV_SVG + LOGO_SVG))
echo "index.html: $INDEX"
echo "styles.css: $CSS"
echo "fonts.css : $FONTS_CSS"
echo "fonts/*   : $FONTS"
echo "favicon   : $FAV_SVG"
echo "logo      : $LOGO_SVG"
echo "TOTAL     : $TOTAL bytes"
[ "$TOTAL" -le 286720 ] && echo "OK (under 280 KB budget)" || echo "FAIL: over budget"
```

Expected: prints `OK (under 280 KB budget)`. If over, the most likely culprit is unused Plex weights; drop one or two weight files and remove the corresponding `@font-face` rules from `fonts.css`.

- [ ] **Step 2: Manual visual check (final pass)**

```bash
python3 -m http.server -d site 8765 &
sleep 1
xdg-open http://localhost:8765/ || open http://localhost:8765/
# Click through: landing, /docs/, /docs/cli.html, /docs/ralphex-compat.html
# Confirm: pixel-grid logo top-left, monochrome theme, IBM Plex fonts loaded,
# spec-sheet hero with two columns, capabilities table, dark code blocks.
kill %1 2>/dev/null
```

- [ ] **Step 3: No commit needed** — this is a verification task.

---

## Verification Gates (before merge)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

All must pass. `tests/site_copy.rs` must show ≥ 24 tests (the existing 17 + the 7 new ones).

## Acceptance Recap

- Landing page renders the spec-sheet hero with monochrome IBM Plex type, pixel-grid logo, no `#00d992`, no Google Fonts CDN.
- All `site/docs/*.html` pages share the new shell.
- `favicon.svg` + `favicon.ico` render the 9-cell pattern in browser tabs.
- `cargo test --all` is green including the 7 new `tests/site_copy.rs` assertions.
- Page weight ≤ 280 KB.
