#!/usr/bin/env python3
"""Build the guide: wrap each chapter fragment in guide/src/<slug>.html with the shared
header, chapter sidebar, "On this page" list and previous/next links, into public/docs/<slug>.html.

A fragment starts with <h1> and holds the chapter's body. Every <h2> needs an id; those become
the "On this page" list. The first <p class="lede"> becomes the page description.
"""
import html, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "src")
DOCS = os.path.join(HERE, "..", "public", "docs")

CHAPTERS = [
    ("Start here", [("index", "How Dalíca works")]),
    ("Your library", [("library", "Your library on disk"), ("scanning", "Scanning and watching"), ("thumbnails", "Thumbnails")]),
    ("Editing", [("recipes", "Edits are recipes"), ("auto-fixes", "Auto Fix, Contrast and Color"), ("tuning", "Tuning and the neutral picker"),
                 ("crop-straighten-retouch", "Crop, straighten and retouch"), ("effects", "The effects"), ("color", "Color and file formats")]),
    ("Organizing", [("metadata", "Metadata in your files"), ("search", "Search, color and duplicates"), ("people", "People"), ("places", "Places"), ("import", "Import")]),
    ("Sharing", [("export", "Export"), ("collage-slideshow", "Collages and slideshows"), ("video", "Video")]),
    ("Under the hood", [("safety", "Never losing a photo"), ("performance", "Big libraries"), ("automation", "Automation")]),
]
ORDER = [s for _, items in CHAPTERS for s in items]

HEAD = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · Dalíca guide</title>
<meta name="description" content="{desc}">
<link rel="icon" href="../img/icon.svg" type="image/svg+xml">
<link rel="stylesheet" href="../style.css">
</head>
<body class="docs-page">
<a class="skip" href="#main">Skip to content</a>
<header class="top stuck" id="top">
  <div class="top-in">
    <a class="brand" href="../" aria-label="Dalíca home"><img src="../img/icon.svg" alt="" width="30" height="30"><span class="wordmark">Dalíca</span></a>
    <nav aria-label="Site">
      <a href="../#tour" class="opt">Tour</a>
      <a href="../#features">Features</a>
      <a href="./" class="current" aria-current="true">Docs</a>
      <a href="https://github.com/pkkid/dalica" class="opt">GitHub</a>
      <a class="button small" href="../#download">Download</a>
    </nav>
  </div>
</header>
<div class="docs">
"""

def sidebar(cur):
    out = ['  <nav class="docs-nav" aria-label="Guide">', '    <details class="docs-nav-fold" open><summary>Chapters</summary>']
    for group, items in CHAPTERS:
        out.append(f'    <p class="docs-group">{group}</p>')
        for slug, title in items:
            href = "./" if slug == "index" else f"{slug}.html"
            exists = os.path.exists(os.path.join(SRC, slug + ".html"))
            if slug == cur:
                out.append(f'    <a href="{href}" aria-current="page">{title}</a>')
            elif exists:
                out.append(f'    <a href="{href}">{title}</a>')
            else:
                out.append(f'    <span title="Coming soon">{title}</span>')
    out.append("    </details>")
    out.append("  </nav>")
    return "\n".join(out)

def build(slug, title):
    src = open(os.path.join(SRC, slug + ".html")).read()
    h2 = re.findall(r'<h2 id="([^"]+)"[^>]*>(.*?)</h2>', src, re.S)
    m = re.search(r'<p class="lede">(.*?)</p>', src, re.S)
    desc = html.escape(re.sub(r"<[^>]+>", "", m.group(1)).strip().replace("\n", " ")) if m else title
    toc = ""
    if h2:
        toc = '  <aside class="docs-toc" aria-label="On this page"><p class="docs-group">On this page</p>\n' + "\n".join(
            f'    <a href="#{i}">{re.sub(r"<[^>]+>", "", t)}</a>' for i, t in h2) + "\n  </aside>\n"
    i = ORDER.index((slug, title))
    nav = '    <nav class="docs-pager" aria-label="Previous and next chapter">'
    if i > 0:
        ps, pt = ORDER[i - 1]; nav += f'<a class="prev" href="{"./" if ps == "index" else ps + ".html"}"><span>Previous</span>{pt}</a>'
    else:
        nav += "<span></span>"
    if i + 1 < len(ORDER):
        ns, nt = ORDER[i + 1]; nav += f'<a class="next" href="{ns}.html"><span>Next</span>{nt}</a>'
    nav += "</nav>"
    page = HEAD.format(title=html.escape(title), desc=desc) + sidebar(slug) + "\n" + \
        f'  <main class="docs-main" id="main">\n    <article class="doc">\n{src}\n    </article>\n{nav}\n  </main>\n' + toc + \
        '</div>\n<div class="lightbox" id="lightbox" role="dialog" aria-modal="true" aria-label="Enlarged picture" hidden><img alt=""><div class="lb-bar"><button type="button" class="lb-size" aria-pressed="false">Actual size</button><span class="lb-hint">Scroll or drag to look around</span><button type="button" class="lb-close" aria-label="Close">Close <kbd>Esc</kbd></button></div></div>\n' + \
        '<script src="../main.js"></script>\n</body>\n</html>\n'
    open(os.path.join(DOCS, slug + ".html" if slug != "index" else "index.html"), "w").write(page)

for slug, title in ORDER:
    if os.path.exists(os.path.join(SRC, slug + ".html")):
        build(slug, title)
        print("built", slug)
