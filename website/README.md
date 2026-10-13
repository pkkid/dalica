# dalica.org

The project's website: one static page that says what Dalica is, hands visitors the right
download and sends bug reports to GitHub. Plain HTML, CSS and JavaScript with no build step and no
third-party requests; Cloudflare serves the files in `public/` as they are.

| Path | What |
|---|---|
| `public/index.html` | the page |
| `public/style.css` | all styles, including the self-hosted `@font-face` rules |
| `public/main.js` | the download button that picks the visitor's system, the animated hex grid behind the page, the count-up numbers, the scroll reveals and the click-to-enlarge screenshots |
| `public/404.html` | the not-found page |
| `public/_headers` | security headers (a strict Content-Security-Policy: everything is served from this site) and cache lifetimes |
| `public/img/` | screenshots of Dalica, the link-preview image `og.png`, the app icon and one photo |
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

1. **Domain:** `dalica.org` must be an active zone in the Cloudflare account (Cloudflare Registrar,
   or the registrar's nameservers pointed at Cloudflare).
2. **Connect the repository:** Workers & Pages → Create → Import a repository → `pkkid/dalica`.
   Root directory `website`, no build command, deploy command `npx wrangler deploy`, production
   branch `main`. Set the build watch paths to `website/**` so code changes never redeploy the
   site; other branches get preview URLs.
3. **Custom domains:** on the Worker, Settings → Domains & Routes → add `dalica.org` and
   `www.dalica.org`. Then Rules → Redirect Rules: `www.dalica.org/*` → `https://dalica.org/${1}`
   (301).
4. **HTTPS:** SSL/TLS → Edge Certificates → Always Use HTTPS. Turn on HSTS once the site has run
   on HTTPS for a while.

`npx wrangler deploy` from this folder deploys by hand.

## Editing the page

- The page is dark only and uses one accent, Gruvbox orange (`#fe8019`, and `#d65d0e` for darker
  uses). Buttons stay quiet greys; screenshots have a 3 px corner radius.
- On this website, and only here, the name is spelled **Dalíca** in visible text. Everywhere else
  (code, docs, the app) it is Dalica, and addresses keep `dalica` (`dalica.org`, `dalica-cli`).
- Name only Dalica's own features. The rules on naming other software are in `AGENTS.md`.
- The numbers in "Fast where it counts" come from the project's measurements (`docs/status.md`,
  `docs/roadmap.md` → Where we stand) and the command registry (`dalica-cli commands`). Update them
  when those change, and update the roadmap timeline when a milestone closes.
- The version (`0.1.0`) appears in the hero button and the download section.

## Images

Screenshots are real captures of Dalica, taken headlessly with `dalica-cli snapshot` at 2× scale
and cropped. The photos inside them, and `img/castle.jpg`, come from the `photos` set of the
project's own corpus ([pkkid/dalica-corpus](https://github.com/pkkid/dalica-corpus) at
`6239cad`), where every file is CC0 or public domain with its source listed in `SOURCES.tsv`.
Never use personal photos. Images, like the fonts, have rows in `ATTRIBUTION.md`.

Images and fonts are cached for a week and a month (`_headers`), so give a replaced image a new
file name.
