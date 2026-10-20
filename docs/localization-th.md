# Thai style guide (`th`)

How Dalica's interface is written in Thai: the catalog `crates/ui/src/i18n/th.tsv`, its rules in
[`localization.md`](localization.md), its text path in [`thai-text.md`](thai-text.md). Every entry
is translated from the meaning of Dalica's English text (`localization.md` → Clean-room
translations); no other product's Thai localisation was consulted.

> Status: **a machine-assisted draft (M15.35).** Every entry is marked `~` (a draft) until a fluent
> speaker has reviewed it; the review is planned for M15.R. Reviewers: check each entry against the
> English and this guide, fix it, delete its `~`, and change this guide where a term is better
> said another way (then change every entry that uses it).

## Register

- **Polite and neutral**, as Thai software is written: no particles (ครับ, ค่ะ, นะ), no slang,
  no royal or literary vocabulary. Address the user as **คุณ** only where a sentence needs a
  subject ("รูปภาพของคุณ"); most labels and instructions have none.
- **Commands and buttons** are short verb phrases (เปิด, บันทึกเป็น…, ย้ายไปที่ถังขยะ).
  **Settings and headings** are noun phrases (ขนาดภาพย่อ, การตั้งค่าโปรแกรมดูรูปภาพ).
- **Questions** end with **หรือไม่** and no question mark ("ลบอัลบั้มนี้หรือไม่"). **Sentences**
  end without a full stop; two sentences in one message are separated by a space, as Thai writes
  them.
- **Progress** reads กำลัง… ("กำลังส่งออก…"); a **finished action** ends with แล้ว ("ย้ายรูปภาพ 3
  รูปแล้ว"); **cannot** is …ไม่ได้ after the verb ("บันทึก {name} ไม่ได้: {reason}").
- No capitalisation exists; Latin words inside Thai keep their own case.

## What stays as written

- **Dalica**, and technology and format names: JPEG, PNG, HEIC/HEIF, RAW, EXIF, XMP, GPS, ISO, HDR,
  MP3, `.onnx`, `https://`, Ctrl, Esc, Enter, Backspace, Page Up and Page Down.
- **User data and placeholders:** folder, file, album and people's names, captions and tags, and
  every `{name}` placeholder (moved to where Thai word order wants it, never dropped).
- **Language names** in the Language list (English, ไทย: each in itself).
- The trailing **…** of a command that opens a dialog, and the arrows and separators of menu paths
  (›, →, ▸).

## Terminology

The core vocabulary, so later entries stay consistent. Dalica's own feature names are rendered by
their meaning.

| English | Thai | Note |
|---|---|---|
| photo, picture | รูปภาพ | counted with **รูป**: "{n} รูป", "รูปภาพ 3 รูป" |
| video, clip | วิดีโอ, คลิป | |
| library | คลังภาพ | the whole collection; *Library* the room too |
| folder | โฟลเดอร์ | counted with โฟลเดอร์ |
| album | อัลบั้ม | |
| tag (noun) / Quick Tags | แท็ก / แท็กด่วน | |
| star / Star (verb) / starred | ดาว / ติดดาว / ติดดาวแล้ว | Star / Unstar: ติดดาว / เอาดาวออก |
| caption | คำบรรยายภาพ | |
| description | คำอธิบาย | |
| thumbnail | ภาพย่อ | |
| Photo Tray / Hold / held | ถาดรูปภาพ / พักไว้ / พักไว้ | the tray under the light table |
| light table | โต๊ะไฟ | |
| Folder Manager | ตัวจัดการโฟลเดอร์ | |
| watched folders | โฟลเดอร์ที่เฝ้าดู | |
| People / person / face | บุคคล / บุคคล / ใบหน้า | Unnamed: ไม่มีชื่อ |
| Places / location / geotag | สถานที่ / ตำแหน่ง / แท็กตำแหน่ง | map: แผนที่; map tiles: ไทล์แผนที่ |
| import / export | นำเข้า / ส่งออก | |
| collage | ภาพปะติด | the Collage room: ห้องภาพปะติด |
| slideshow | สไลด์โชว์ | |
| room | ห้อง | Library, Import and Collage are rooms |
| edit (verb) / edit view | แก้ไข / มุมมองแก้ไข | |
| Auto Fix | แก้ไขอัตโนมัติ | one click for contrast and colour |
| Auto Contrast / Auto Color | ปรับคอนทราสต์อัตโนมัติ / ปรับสีอัตโนมัติ | |
| Crop / Straighten / Retouch | ครอบตัด / ปรับให้ตรง / รีทัช | |
| Fill Light / Highlights / Shadows | เติมแสง / ส่วนสว่าง / ส่วนมืด | |
| Color Temperature | อุณหภูมิสี | |
| Tuning | การปรับแต่ง | the Tuning tab |
| effect | เอฟเฟกต์ | each effect by its meaning (ซีเปีย, ขาวดำ, เกรนฟิล์ม, ขอบมืด, …) |
| Sharpen / Soften | เพิ่มความคมชัด / ทำให้นุ่ม | |
| Undo / Redo | เลิกทำ / ทำซ้ำ | Undo Save: เลิกทำการบันทึก |
| Save / Save As / Revert | บันทึก / บันทึกเป็น / คืนค่า | |
| original | ต้นฉบับ | |
| Delete / Remove / Trash | ลบ / เอาออก / ถังขยะ | *Delete* destroys or trashes a file; *Remove* takes something out of a list, an album or Dalica |
| metadata | ข้อมูลเมตา | |
| Options | ตัวเลือก | the dialog and its tabs |
| keyboard shortcuts / shortcut | แป้นพิมพ์ลัด / ทางลัด | |
| preview | แสดงตัวอย่าง | |
| Cancel / OK / Apply / Close | ยกเลิก / ตกลง / นำไปใช้ / ปิด | |

## Formats

- **Dates:** day, month, year: "{day} {month} {year}" (6 มิถุนายน 2026); with the weekday,
  "{weekday}ที่ {day} {month} {year}" (วันเสาร์ที่ 6 มิถุนายน 2026). Full month names (มกราคม …
  ธันวาคม) and weekday names with วัน (วันจันทร์ …). The year is the Christian Era the formats
  give; showing the Buddhist Era (พ.ศ., +543) would need a change to `i18n::format`, not to the
  catalog (a question for the review).
- **Times:** the formats give a 12-hour clock: "{hour}:{minute} ก่อนเที่ยง" / "หลังเที่ยง".
- **Numbers and sizes:** Western digits, `.` decimal mark, `,` thousands separator (the registry
  row); units as written in English (KB, MB, GB, px), seconds as วินาที, minutes as นาที, hours as
  ชม.
- **Plurals:** Thai has one form (`Plural::Single`); every `@plural` entry lists one translation,
  holding `{n}` with its classifier (รูป, ไฟล์, โฟลเดอร์, ใบหน้า, แห่ง, รายการ, คน).
- **Lists:** joined with ", " as the English does.
- **Line breaks:** Thai writes no spaces between words. Write entries plainly: the catalog's text is
  marked for wrapping between words when it is read (`thai-text.md`). Avoid long loanword
  transliterations where a plain Thai word exists: the word dictionary may split a transliteration
  oddly (ไฮไลต์ was split before ต์, so Highlights reads ส่วนสว่าง).

## Coverage

Checked on the M15.35 branch over `main` at `a7fdedcc` (2026-10-20):

- `cargo xtask i18n`: **1590 of 1590 keys** translated (100 %; `complete_menus`, so a missing key
  fails the gate), **1590 drafts** awaiting the fluent review, no catalog fault. The release notes
  stay in English (16 of 312 lines happen to match a translated heading); they are reported, never
  failed.
- `crates/ui/tests/i18n_languages.rs` with craft-fonts at the release pin (`3d9ab3a`, Noto Sans
  Thai): the library, the edit view (Basic, Tuning, Effects) and all eight tabs of Options at 1400 ×
  900 and 900 × 600 with no label cut, every wrapped Thai label broken between words, every Thai
  character drawn by a face that has it and every mark over or under its consonant; plurals for 0,
  1, 2, 5 and 21; switching English ⇄ Thai keeping the view, the selection and the undo history; the
  library's fingerprint `i18n-th-library-1400x900`. The screenshots were looked at.
- Not covered by a sweep in Thai (only in the pseudo-locale): the other dialogs, the rooms, the
  slideshow and the viewer. The review's hand check covers them, and text input with a Thai input
  method (`thai-text.md` → Known limits).
