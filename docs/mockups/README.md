# Mockups

Target pictures of Dalica's windows, drawn from our own specs (`ui-layout.md` for layout,
`design-system.md` for colours, type and metrics) before the UI exists. They are what the window
should look like, so agents building the shell (M3–M4) aim at one shared image instead of each
reading the text differently. Original work; photos are procedural gradients, icons are our own.

| File | What |
|---|---|
| `library.html` | the library window, generated from the spec numbers (open it in a browser; toggles for 1400 × 900 / 900 × 600 and the Tags pane; `?bare=1&w=…&h=…&pane=none\|tags` for screenshots) |
| `library-1400x900.png` | default size: Lisbon Weekend selected as a section, 3 photos selected, 2 held from Garden |
| `library-900x600.png` | minimum size: action buttons overflow into More…, filter caption and title hidden |
| `library-1400x900-tags.png` | the same with the Tags side pane open |

**Status:** proposal, 2026-10-09; waiting for the maintainer's review. Once approved, M3–M4
compare their screenshots with these at the same sizes. When the spec and a mockup disagree, the
spec wins and the mockup is regenerated (`google-chrome --headless=new --window-size=W,H
--screenshot=… "file://…/library.html?bare=1&w=W&h=H"`).

An edit-view mockup follows before M7.
