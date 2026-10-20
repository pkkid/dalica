<!-- dalica: unsigned builds -->
> **These builds are not code-signed.** The Windows installer and portable zip carry no Authenticode
> signature, and the macOS app is signed ad hoc and not notarized by Apple. Windows and macOS
> therefore warn before the first start. Check your download against `SHA256SUMS.txt` first:
> `sha256sum -c SHA256SUMS.txt --ignore-missing` (macOS: `shasum -a 256 -c SHA256SUMS.txt --ignore-missing`).
>
> **Windows:** if SmartScreen says "Windows protected your PC", click **More info**, then **Run
> anyway**. If Windows then asks for permission, the prompt names an "Unknown publisher"; that is
> expected. For the portable zip, right-click it, choose **Properties**, tick **Unblock** and click OK before you
> extract it. With Smart App Control turned on, Windows 11 blocks unsigned programs without offering
> Run anyway.
>
> **macOS:** open the DMG and drag Dalica to Applications, then open Dalica once. When macOS says it
> cannot check Dalica for malware, click **Done** (not Move to Trash), open **System Settings ›
> Privacy & Security**, scroll to Security, click **Open Anyway** next to Dalica and confirm with
> your password. On macOS 14 and earlier you can instead Control-click Dalica in Applications,
> choose **Open**, then **Open**. You do this once; later starts open normally. If macOS says Dalica
> "is damaged", that is the download's quarantine flag, not damage: run
> `xattr -dr com.apple.quarantine /Applications/Dalica.app` in Terminal and open it again. The same
> applies to `dalica-cli` from the CLI zip (`xattr -d com.apple.quarantine dalica-cli`).
