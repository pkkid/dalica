# French style guide (`fr`)

How Dalica's interface is written in French: the catalog `crates/ui/src/i18n/fr.tsv`, its rules in
[`localization.md`](localization.md). Every entry is translated from the meaning of Dalica's
English text (`localization.md` → Clean-room translations); no other product's French
localisation was consulted.

> Status: **a machine-assisted draft.** Every entry is marked `~` (a draft) until a fluent speaker
> has reviewed it. Reviewers: check each entry against the English and this guide, fix it, delete
> its `~`, and change this guide where a term is better said another way (then change every entry
> that uses it).

## Register

- **Vous**, never *tu*. Neutral, plain software French; no anglicisms where a common French word
  exists (*télécharger*, *étiquette*, *infobulle*), but established terms stay (*pull request*,
  *commit*, *proxy*, *build*).
- **Commands and buttons** are infinitives (*Ouvrir*, *Enregistrer sous…*, *Mettre à la
  corbeille*). **Settings and headings** are noun phrases (*Taille des vignettes*, *Réglages de la
  visionneuse*). **Instructions** use the imperative with *vous* (*Choisissez un dossier*).
- **Capitalisation:** sentence case everywhere, menus included (*Afficher toutes les métadonnées*);
  only the first word and proper names take a capital. Month and weekday names are lower case.
- **Punctuation:** a no-break space before `:` (U+00A0) and a narrow no-break space before `?`,
  `!` and `;` (U+202F), French quotation marks with no-break spaces inside (« … », U+00A0), so a
  line never wraps before the mark; the trailing `…` kept on commands that open a dialog.
- **Finished actions** are past participles agreeing with the counted noun (*3 photos déplacées*);
  **cannot** reads *Impossible de …* (*Impossible d'enregistrer {name} : {reason}*).

## What stays as written

- **Dalica**, and technology and format names: JPEG, PNG, HEIC/HEIF, RAW, EXIF, XMP, GPS, ISO, HDR,
  MP3, `.onnx`, `https://`, and key names (Ctrl, Alt, Esc, Enter, Backspace), as the menus and
  the Keyboard Shortcuts editor write them.
- **User data and placeholders:** folder, file, album and people's names, captions and tags, and
  every `{name}` placeholder (moved to where French word order wants it, never dropped).
- **Language names** in the Language list (each in itself: English, ไทย, Français).
- Theme names that are proper names (Adwaita, Breeze, Solarized, Pro, Studio), with their variant
  translated (*Breeze (sombre)*).

## Terminology

| English | French | Note |
|---|---|---|
| photo, picture | photo | *picture* and *photo* both read *photo*; *image* for a video frame or a generic bitmap |
| video, clip | vidéo, clip | |
| library | photothèque | the room too: *Photothèque* |
| folder / watched folders | dossier / dossiers surveillés | |
| album | album | |
| tag / Quick Tags | étiquette / étiquettes rapides | |
| star / Star / starred | étoile / ajouter l'étoile / étoilées | Star / Unstar: *Ajouter / retirer l'étoile* |
| caption / description | légende / description | |
| thumbnail | vignette | |
| Photo Tray / Hold / held | bac photo / mettre de côté / mises de côté | |
| light table | table lumineuse | |
| Folder Manager | Gestionnaire de dossiers | |
| People / person / face | Personnes / personne / visage | Unnamed: *Sans nom* |
| Places / location / geotag | Lieux / lieu / géolocalisation | map tiles: *tuiles de carte* |
| import / export | importer, importation / exporter, exportation | |
| collage / slideshow | collage / diaporama | |
| room | espace | Library, Import and Collage are *espaces* |
| edit (verb) / edit view | retoucher / vue Retouche | an edit: *une retouche* |
| Auto Fix | Correction automatique | |
| Auto Contrast / Auto Color | Contraste auto / Couleurs auto | |
| Crop / Straighten / Retouch | Recadrer / Redresser / Retouche locale | Retouch's patches: *retouches locales* |
| Fill Light / Highlights / Shadows | Lumière d'appoint / Hautes lumières / Ombres | |
| Tuning | Réglages fins | |
| Sharpen / Soften | Netteté / Adoucir | |
| Undo / Redo | Annuler / Rétablir | Undo Save: *Annuler l'enregistrement* |
| Save / Save As / Revert | Enregistrer / Enregistrer sous / Revenir à l'original | |
| Delete / Remove / Trash | Supprimer / Retirer / corbeille | *Supprimer* destroys or trashes a file; *Retirer* takes something out of a list, an album or Dalica |
| sidecar | fichier annexe | |
| metadata | métadonnées | |
| Options / keyboard shortcuts | Options / raccourcis clavier | |
| photo viewer | visionneuse | |
| Stats for Nerds | Statistiques pour geeks | |
| Cancel / OK / Apply / Close | Annuler / OK / Appliquer / Fermer | |

## Formats

- **Dates:** day, month, year: *4 juin 2026*; with the weekday, *jeudi 4 juin 2026*; ranges *du 4
  juin 2026 au 6 juin 2026*.
- **Typed dates:** the date field writes and reads `{day}/{month}/{year}` (31/12/2026); the time
  field stays `HH:MM:SS`, 24 hours.
- **Times:** the 24-hour clock (`clock24` in the registry row): `{hour}` is 00 to 23, so the AM and
  PM templates read alike, *15:05*, *4 juin 2026, 15:05*.
- **Numbers and sizes:** a decimal comma (`3,5`) and a narrow no-break space (U+202F) between
  thousands (`1 234 567`), from the registry row; bytes as *octets* and units as Ko, Mo, Go, To.
- **Plurals:** `Plural::ZeroOneOther`: the singular for 0 and 1 (*0 photo*, *1 photo*, *2 photos*).

## Coverage

- `cargo xtask i18n`: **1730 of 1730 keys** translated (`complete_menus`, so a missing key fails
  the gate), **1762 drafts** (the keys, the command labels a macro makes, and engine messages), no
  catalog fault. The release notes stay in English.
- `crates/ui/tests/i18n_languages.rs`, a row per language: the catalog reads whole, every command
  has a label, plurals for 0, 1, 2 and 21, numbers, sizes, dates and times, `fr-CA` and `fr_BE`
  resolve to French; at full strength (`DALICA_FULL_TESTS=1`) the library, the edit view and
  Options drawn in French at 1400 × 900 and 900 × 600 with no label cut.
