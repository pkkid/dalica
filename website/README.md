# dalica.pkarma.com

The project's website: one static page that says what Dalica is, hands visitors the right
download and sends bug reports to GitHub. Plain HTML, CSS and JavaScript with no build step and no
third-party requests; Cloudflare serves the files in `public/` as they are.

| Path | What |
|---|---|
| `public/index.html` | the page |
| `public/style.css` | all styles, including the self-hosted `@font-face` rules |
| `public/main.js` | the release on offer (`RELEASE`: version and file sizes) and the download links built from it, the download button that picks the visitor's system, the animated hex grid behind the page, the count-up numbers, the scroll reveals and the click-to-enlarge screenshots |
| `public/404.html` | the not-found page |
| `public/_headers` | security headers (a strict Content-Security-Policy: everything is served from this site) and cache lifetimes |
| `public/img/` | screenshots of Dalica, the link-preview image `og.png`, the app icon, one photo, and the CC0 licence of the system logos inlined in the page |
| `public/fonts/` | Schibsted Grotesk and IBM Plex Mono, with their OFL licence texts |
| `wrangler.jsonc` | the Cloudflare configuration: a Worker with static assets only |

## Look at it locally

```sh
python3 -m http.server 8000 --directory website/public
```

Then open <http://localhost:8000>. The page uses root-relative paths (`/style.css`), so serve the
`public` folder as the web root rather than opening `index.html` as a file.

## Deploy to Cloudflare

The site is a Worker with static assets on the Free plan. Installers are not hosted here: the
download links point at GitHub Releases, which keep the files and their checksums.

1. **Domain:** `pkarma.com` must be an active zone in the Cloudflare account. `wrangler.jsonc`
   attaches `dalica.pkarma.com` as the Worker's custom domain, so a deploy creates its DNS record
   and certificate; there is nothing to add by hand.
2. **Connect the repository:** Workers & Pages → Create → Import a repository → `pkkid/dalica`.
   Root directory `website`, no build command, deploy command `npx wrangler deploy`, production
   branch `main`. Set the build watch paths to `website/**` so code changes never redeploy the
   site.
3. **HTTPS:** Always Use HTTPS is on for the `pkarma.com` zone. Turn on HSTS once the site has run
   on HTTPS for a while.

`npx wrangler deploy` from this folder deploys by hand.

## Editing the page

- The page is dark only and uses one accent, Gruvbox orange (`#fe8019`, and `#d65d0e` for darker
  uses). Buttons stay quiet greys; screenshots have a 3 px corner radius.
- On this website, and only here, the name is spelled **Dalíca** in visible text. Everywhere else
  (code, docs, the app) it is Dalica, and addresses keep `dalica` (`dalica.pkarma.com`, `dalica-cli`).
- Name only Dalica's own features. The rules on naming other software are in `AGENTS.md`.
- The numbers in "Fast where it counts" come from the project's measurements (`docs/status.md`,
  `docs/roadmap.md` → Where we stand) and the command registry (`dalica-cli commands`). Update them
  when those change.
- The road to 1.0 lists every release with the roadmap's own words (`docs/roadmap.md` →
  Overview). When a release ships, move its card from "Next" to "Shipped" (`class="done"`) and
  mark the following one `class="soon"`.
- The Linux, Apple and Windows logos are Simple Icons path data (CC0), inlined as `<symbol>`s at
  the top of `index.html`; the download arrow and the note icons are drawn for this page.

## Releasing a new version

Every milestone ends in a release (`docs/roadmap.md` → How milestones work), and the page offers
exactly one. The version lives in one place, `RELEASE` at the top of the download code in
`public/main.js`; the links and labels in the HTML hold no copy of it. Each download link names its file with `{v}` for the
version (`data-file="dalica-{v}-windows-x64.msi"`), and the script fills in the address, the file
name, the size and every "Preview 0.x" label. Without JavaScript, the links open the latest
release's page on GitHub instead.

1. Once the release is published on GitHub, print its sizes (for 0.3.0):

   ```sh
   gh release view v0.3.0 -R pkkid/dalica --json assets \
     --jq '[.assets[] | {key: (.name | sub("0\\.3\\.0"; "{v}")), value: .size}] | from_entries'
   ```

2. In `RELEASE`, set `version: "0.3.0"` and replace `sizes` with that output.
3. If the release added, renamed or dropped a file, change its row in the download section.
4. Check every link: serve the page, or run
   `for f in $(grep -o 'data-file="[^"]*"' public/index.html | cut -d'"' -f2 | sed 's/{v}/0.3.0/'); do curl -sIL -o /dev/null -w "%{http_code} $f\n" https://github.com/pkkid/dalica/releases/download/v0.3.0/$f; done`
   (every line should say 200).
5. Update the road to 1.0, the feature grid and "Coming next" if the release changed them, and
   the numbers from the review's *Where we stand*.

## Images

Screenshots are real captures of Dalica, taken headlessly with `dalica-cli snapshot --size
2800x1800 --scale 2` and cropped. For the 0.2 shots, a library was built with `dalica-cli run`
from copies of corpus photos in four folders (`folders.add`, `folder.info.set`,
`photo.tags.add`, `photo.star.set`, a crop, a straighten, a tuning and Auto Fix), then shown with
script steps such as `{"command": "view.edit", "params": {"id": 3}}` and
`{"command": "edit.tool.open", "params": {"tool": "crop"}}`. The hero shot, `library.jpg`, is a library of
25 corpus photos in four folders (Highlands, Spring, Old towns, By train), made with `folders.add`
and `folder.info.set` (a date and a description per folder) and snapshotted with no script, so no
dialog or notice is open; it is resized to 2000 × 1286. The 0.3 shots (`crop-save.jpg`,
`crop-export.jpg`, `crop-projects.jpg`) use the generated demo library
(`dalica-cli snapshot --demo-fresh`): a crop on two photos of the Hills folder, then steps such as
`{"command": "photo.save", "params": {"ids": [25, 27]}}` (which opens the Save dialog),
`export.run` with a size, numbers and a watermark, and `output.export` with
`output.export.location`. Run them with `HOME` and the `XDG_*` folders pointed at a scratch
folder, so no real path shows in the Export dialog. The export progress window is caught while
the export is still running: export all 60 demo photos so the last frame comes before it ends.
The five feature shots of 0.5 (`feat-*.jpg`, 1500 × 1000, 3:2) use a library like the hero's (25 corpus
photos in Highlands, Spring, Old towns and By train) with tags, stars, two captions and an album
(`album.create`), snapshotted at `--size 2800x1800 --scale 2` (the edit view at `2280x1800`, so its crop
is 3:2) with steps such as `{"command": "search.set", "params": {"text": "mountains", "filters":
{"starred": true}}}`, `edit.autoFix`, `scroll.nextSection` and `output.export` with
`output.export.location` and clicks on `check:dialog.numbers` and `check:dialog.watermark`.
The four 0.6 shots (`feat-import.jpg`, `feat-compare.jpg`, `feat-slideshow.jpg`, `feat-viewer.jpg`,
1500 × 1000) were taken from the running app, not `dalica-cli snapshot`: `dalica --headless --control`
(and `dalica --view FILE --headless` for the viewer) with `interface.uiScale` at 200 and `ui.resize`
to 1350 × 900 points, so `ui.screenshot` writes 2700 × 1800, then resized. The library is the 25 corpus
photos in Highlands, Spring, Old towns and By train again, with stars, tags, captions and folder
dates and descriptions. The import source is a folder `Card/DCIM/100DALIC` of eleven corpus photos
renamed `DSC_41xx.jpg`, their metadata stripped and their file times set to three sessions so they
group (`import.openSource {path}`, `import.exclude`, `import.mark`, `import.destination`,
`import.select`); compare is `view.edit`, `edit.compare {"mode": "same"}`, `edit.switchFocus` and
`edit.autoFix`; the slideshow is `section.slideshow` with captions on and `slideshow.pause`, which
shows the control bar; the viewer has `viewer.hideControls` off and is shot once two captures in a
row are the same, so the filmstrip's thumbnails are in.
The three 0.7 shots (`feat-effects.jpg`, `feat-effect-panel.jpg`, `feat-frames.jpg`, 1500 × 1000) use the
generated demo library: `dalica-cli snapshot --demo-fresh --size 1800x1200 --scale 2` (a compact 900 × 600 window, so the small demo photos read), resized. Each script opens a
Hills photo (`view.edit` with id 22 or 26), picks an effects tab (`edit.tab` with `effects` or `effects3`), opens an
effect's panel (`edit.tool.open {"tool": "effect", "name": "graduated_tint"}`), or applies one
(`museum_matte`, then `edit.tool.apply`), and zooms in one step with `edit.zoom.in` (the demo photos are 480 × 320).
The photos inside the 0.2, 0.5 and 0.6 shots and `library.jpg`, and `img/castle.jpg`, come from the `photos` set of the
project's own corpus ([pkkid/dalica-corpus](https://github.com/pkkid/dalica-corpus) at
`6239cad`), where every file is CC0 or public domain with its source listed in `SOURCES.tsv`.
Never use personal photos. Images, like the fonts, have rows in `ATTRIBUTION.md`.

Images and fonts are cached for a week and a month (`_headers`), so give a replaced image a new
file name.
