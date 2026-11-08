# dalica.pkarma.com

The project's website: a one-page overview of what Dalica is, with the download, and the guide
(`/docs/`), which explains what happens inside Dalica that the window does not show. Plain HTML,
CSS and JavaScript with no build step for the page and no third-party requests; Cloudflare serves
the files in `public/` as they are.

| Path | What |
|---|---|
| `public/index.html` | the page: hero, the six-step tour (one screenshot over a filmstrip), the feature tabs, "Your files stay yours", download, FAQ |
| `public/style.css` | all styles, for the page and the guide, including the self-hosted `@font-face` rules |
| `public/main.js` | the download button that names the visitor's system, the tour's filmstrip viewer, the feature tabs, the quiet reveals, the lightbox (fit, actual size, drag to pan), and the guide's "On this page" highlight |
| `public/docs/` | the guide, built from `guide/src/` (below); `docs/img/<chapter>/` holds each chapter's figures |
| `public/credits.html` | every photo shown in the screenshots and the guide, with its author, licence and page |
| `public/404.html` | the not-found page |
| `public/_headers` | security headers (a strict Content-Security-Policy: everything is served from this site, and no inline styles or scripts) and cache lifetimes |
| `public/img/` | screenshots of Dalica and the tour's thumbnails, the app icon, the link-preview image `og.png`, and the CC0 licence of the system logos inlined in the page |
| `public/fonts/` | Schibsted Grotesk and IBM Plex Mono, with their OFL licence texts |
| `guide/src/<chapter>.html` | each guide chapter as an HTML fragment (its `<h1>`, a `<p class="lede">` and the body) |
| `guide/build.py` | wraps every fragment in the guide's page (header, chapter list, "On this page", previous and next) into `public/docs/` |
| `wrangler.jsonc` | the Cloudflare configuration: a Worker with static assets only |

## Look at it locally

```sh
python3 -m http.server 8000 --directory website/public
```

Then open <http://localhost:8000>. Every link is relative, so the site works from any folder.

## Deploy to Cloudflare

The site is a Worker with static assets on the Free plan. Installers are not hosted here: the
download links point at GitHub Releases, which keep the files and their checksums.

1. **Domain:** `pkarma.com` must be an active zone in the Cloudflare account. `wrangler.jsonc`
   attaches `dalica.pkarma.com` as the Worker's custom domain, so a deploy creates its DNS record
   and certificate; there is nothing to add by hand.
2. **Deploy by hand:** from this folder, once the change is merged to `main`, run
   `npx wrangler deploy` (logged in with `npx wrangler login`). The site is never deployed from
   GitHub: there is no Workers Builds connection, so pushing to `main` changes nothing live.
3. **HTTPS:** Always Use HTTPS is on for the `pkarma.com` zone. Turn on HSTS once the site has run
   on HTTPS for a while.

After a deploy, check the live site: `curl -sI https://dalica.pkarma.com/` and
`curl -sI https://dalica.pkarma.com/docs/` should both answer 200.

## Editing the page

- Dark Gruvbox only: one warm accent (orange `#fe8019`, `#d65d0e` for darker uses) and one cool
  one (blue-gray `#83a598`); the colors are tokens at the top of `style.css`. Screenshots sit in a
  `#313131` mat, the app's own window gray.
- On this website, and only here, the name is spelled **Dalíca** in visible text. Everywhere else
  (code, docs, the app) it is Dalica, and addresses keep `dalica` (`dalica.pkarma.com`, `dalica-cli`).
- Name only Dalica's own features, and photo-management ones first. The rules on naming other
  software are in `AGENTS.md`.
- No `style="…"` attributes and no inline scripts: the Content-Security-Policy in `_headers` refuses
  them. A value an element needs comes from a small class (`.cols-3`, `.d35`, see the end of
  `style.css`).
- Animations stay quiet: short, eased, and off under `prefers-reduced-motion`.
- Every page fits a 320 px phone with no sideways scroll, the guide included: long code, file
  paths and recipe labels wrap or scroll inside their own box, never the page. Check new content
  at phone width (a mobile viewport's `document.documentElement.scrollWidth` equals its width).
- Every claim must hold for the current release. Check it against the code, and the specs in
  `docs/`, before it goes on the page.

## The guide

- Each chapter is a fragment in `guide/src/`. Edit it there, then run `python3 website/guide/build.py`,
  which writes every page of `public/docs/`. The chapter list and its order are `CHAPTERS` at the
  top of `build.py`; a chapter without a fragment shows as "Coming soon".
- The guide explains the machinery, not the buttons: the algorithms, the numbers, the files, the
  order of operations. Facts come from the code; where a spec in `docs/` disagrees, the spec is
  fixed to match the code.
- Building blocks a fragment may use: `<h2 id="…">` (every one needs an id; they make "On this
  page"), `<h3>`, `<ol class="steps-list">`, `<dl class="terms">`, `<div class="note">` (and
  `note warn`), `<pre><code>`, `<div class="table-wrap"><table>`, `<figure>` with one image or a
  `<div class="row cols-N">` of images each followed by a `<span class="label">`, and inline SVG
  diagrams (with a `viewBox`, `role="img"` and an `aria-label`). Every image needs an `alt` text and
  a caption.
- Pictures that show an edit are Dalica's own output: the photo's recipe set with `edit.recipe.set`
  and exported with `export.run` through `dalica-cli`, never an imitation. Charts are computed from
  those renders.

## Releasing a new version

The version appears in `public/index.html` only: the "New in" pill under the headline, the
download section's heading, and the download links (`releases/download/v1.1.0/dalica-1.1.0-…`).
When a release ships:

1. Print its file sizes: `gh release view v1.2.0 -R pkkid/dalica --json assets --jq '.assets[] | "\(.name) \(.size)"'`.
2. Replace `1.1.0` and `1.1` with the new version in the links and labels, update the sizes on the
   three download buttons, and say what is new in the pill (link it to the release's notes).
3. Check every link: `for u in $(grep -o 'https://github.com/pkkid/dalica/releases/download/[^"]*' website/public/index.html); do curl -sIL -o /dev/null -w "%{http_code} $u\n" "$u"; done` (every line should say 200).
4. If the release changed what a chapter of the guide describes, update the chapter.

## Images

Screenshots are real captures of Dalica 1.1, made headlessly: `dalica-cli snapshot --library … --size
2400x1560 --scale 2` (a 1200 × 780 window), or, for the Import room and the Places pane, the
running app with `dalica --headless --control` and `ui.screenshot {scale: 2}` (an import listing and
map tiles arrive while the app runs). The library holds 56 openly licensed photos from Wikimedia
Commons in five folders ("Iceland ring road", "Kyoto", "Garden in June", "Backyard foxes", "Cape
May") and a card of 12 more, with dates set from each photo's capture time, captions, tags, stars,
places and a "Wildlife" album, built with `folders.add`, `photo.dates.set`, `photo.caption.set`,
`photo.tags.add`, `photo.star.set`, `photo.geo.set` and `album.create`. Each shot is cropped to the
part its step is about (the title bar always removed), resized to 1800 px wide and saved as JPEG.
Run Dalica with `HOME` and the `XDG_*` folders pointed at a scratch folder, so no real path shows
(the Export dialog's location is set with `output.export.location`).

Every photo shown, here or in the guide, is listed on `credits.html` with its author and licence
(CC BY 4.0, CC BY-SA 4.0 or CC0), and the images have rows in `ATTRIBUTION.md`. Never use personal
photos. Images and fonts are cached for a week and a month (`_headers`), so give a replaced image a
new file name.
