# Tinta PDF — Tinta Office roadmap

> **Status:** 2026-10-30 · approved by the owner (Denys) · **Revision history:** 2026-10-27 created; 2026-10-27 synced with upstream (#13), Help › Report a problem and issue forms added; government forms harness (#15); OCR in 12 more languages; 2026-10-28 timestamp servers (#17), Windows root trust; OCR tested on phone photos, column split and confidence filter; 2026-10-30 Catalan textbook photos, sideways pages read upright; Ukrainian photos, text found in tiles; Spanish contract photos, tile repeats and slanted lines fixed.

Tinta PDF is the Tinta Office build of PdfCraft and replaces Acrobat Pro. Upstream keeps the
feature-parity roadmap ([ROADMAP.md](../ROADMAP.md), [roadmap.md](roadmap.md),
[gaps.md](gaps.md)); this page lists only what the fork does and which upstream gaps matter most
for users in Spain and Ukraine. It is a fork-only file, so upstream syncs never conflict on it.

## Where it stands (2026-10-27)

Ready for real work ≈ 50% (essentials user ≈ 66%, upstream estimate); 203 commits behind
upstream. Viewing, comments, pages, forms and protection are at 65–70%. Missing for Spain: OCR
now reads Spanish, Catalan, Galician, Basque, Ukrainian and 7 other languages (PaddleOCR models, fork), tested on phone photos of Spanish flyers and a Catalan textbook; there is no DNIe / smart-card signing.

## Priorities

| # | Task | Where | Stage |
|---|---|---|---|
| 1 | Signing with an FNMT certificate from the Windows store: test with a real certificate that Acrobat Reader, AutoFirma and VALIDe accept the signature; add a timestamp (TSA). **Checked 2026-10-28:** an ACCV certificate signature is valid in VALIDe and Acrobat; timestamps from ACCV, DigiCert and FreeTSA work (`tsa` on `sign_document`, `sign_timestamp`); Tinta trusts the Windows root store so it shows such signatures as valid itself. the Sign dialog has a Timestamp option (ACCV server by default). Next: AutoFirma, an FNMT certificate | upstream + fork | 1 |
| 2 | OCR in Spanish, Catalan and Ukrainian (upstream gap 7). **Done in the fork:** PaddleOCR PP-OCRv5 Latin + Cyrillic models, 13 languages, Unicode text layer. **Tested 2026-10-28 on four phone photos of Spanish flyers:** body text and accents read almost perfectly. Fixed then: words of different columns or table cells on one line ran together (now read apart, with the gaps read too for words the detector missed), and pictures were read as junk words (now dropped below a 0.7 confidence). **Tested 2026-10-30 on three phone photos of a Catalan textbook:** Catalan letters (à è é í ò ó ú ç l·l) read correctly. Fixed then: pages photographed sideways read nothing (now read turned upright when most word boxes stand up; 1 → 484 and 2 → 620 words), and words split where the detector missed a few letters (columns are now split only at gaps of 3+ line heights). **Tested 2026-10-30 on eleven phone photos of Ukrainian documents and a children's book:** the detection model shrinks any image to 600 × 800, so on a page photographed whole the small print was lost; a larger image is now also searched in tiles of 1200 × 1600 (reading is 2–4× slower). **Tested 2026-10-30 on three phone photos of a Spanish electricity contract:** the tiles had read lines twice where a box of the whole image crossed the tiles' boxes, so the earlier count (40–170% more words) was mostly repeats, and a page of three dense columns photographed at a slant lost almost all its text (250 → 18 words), because a slanted line's box held the lines above and below. Fixed: a whole-image box that crosses the tiles' boxes is dropped, and a slanted line is read in pieces that stay level. On all 25 test photos, 19% more words than before the tiles, without repeats (the contract's column page 250 → 459, the densest Ukrainian page 34 → 212). Still weak: the Cyrillic model reads і/ї/є as и/і/е most of the time (a Ukrainian word list could correct it), Latin look-alikes inside Cyrillic words, two-language columns side by side mixed, stamps and guilloche backgrounds, large light-on-dark headings, i/l confusions in small print, lines cut at the spine of a book spread, upside-down pages, and `page_enhance_camera` makes OCR of such photos worse; next: Ukrainian samples, offer upstream | fork → upstream | 1–2 |
| 3 | Government forms: a set of public AEAT, Seguridad Social and DGT forms — fill, save, print; bugs found go upstream. **Harness done** (`crates/automation/tests/tinta_gov_forms.rs`, forms kept outside the repo in `PDFCRAFT_GOV_FORMS_DIR`); 8 forms, no engine bug found yet | our tests, fixes upstream | 1 |
| 4 | Create PDF from .docx/.xlsx/.pptx and Word export that keeps layout (upstream rank 0, #773); in the suite this can go through Texto/Calculo/Presenta | upstream + suite integration | 2 |
| 5 | DNIe and cards (PKCS #11), online revocation (LTV), the eIDAS trusted list | upstream (M9) | 2 |
| 6 | Editing existing text (≈ 30% today) | upstream (M7) | 2–3 |
| 7 | PDF/A for archiving and filing with courts and administrations | upstream (M11) | 3 |

Not ours: the own renderer and fidelity harness, accessibility and Preflight are large upstream
work; we report files that render wrongly.

## Suite-wide work (all five Tinta Office apps)

Stages: **1** = November 2026 (Preview 3), **2** = December 2026 – January 2027 (public beta of
the most ready apps), **3** = 2027 (1.0). Dates are indicative.

| Work | Stage | What |
|---|---|---|
| Upstream sync | ongoing | Weekly, Mondays. Catch up before Preview 3. |
| Our changes upstream | 1 | Offer storytold the configurable brand, Presenta's translations, the codec feature flag and the ca/gl/eu catalogs (craft-i18n#53), so the fork stays small. |
| GitHub Actions | 1 | GitHub disabled Actions in pdfcraft, wordcraft and deckcraft; ask GitHub Support. Until then, run the gates locally. |
| Releases | 1 | A preview every month for all five apps, release notes in es/uk/en, one build script. |
| Linux | 1 | AppImage + .deb with Tinta names for all five (built in the cloud); Flatpak in stage 2. **PDF: test build done 2026-10-28** (`packaging/linux/package.sh` already applies the Tinta identity). Built in the cloud with `cargo zigbuild --target x86_64-unknown-linux-gnu.2.35` so it runs on glibc 2.35 and newer (Ubuntu 22.04+, Debian 12+, Fedora 36+), then `package.sh --skip-build` with `CROSS_TARGET=x86_64-unknown-linux-gnu`; the .deb installs, finds the OCR models and the AppImage opens (checked under Xvfb). On X11 the app needs libxkbcommon-x11 (now recommended by the packages). |
| Windows installers | 1 | Desktop shortcuts for Texto and Calculo; Presenta registered for .pptx. |
| Code signing | 2 | Windows (Certum Open Source or Azure Trusted Signing; scripts exist), macOS (Apple Developer + notarization). Owner decision and payment. |
| Automated tests | 1–2 | Coverage audit per app; tests for every fork change (brand, languages, installers); a smoke test of each release installer on Windows; investigate the flaky tests upstream. |
| User guide | 2 | A short "first steps" guide per app in Spanish and Ukrainian on tintaoffice.com (Correo has one). |
| Fork documentation | 1 | This file: what the fork changes, how to build a release, how to sync. |
| Translations | 1–2 | Ukrainian reviewed by the owner, Spanish by a native tester, ca/gl/eu through communities. Marked as drafts until reviewed. |
| Feedback | 1 | Help › Report a problem → the fork's issues or hola@tintaoffice.com; issue templates in Spanish and Ukrainian. **PDF: done** (`help.report` opens `PDFCRAFT_BRAND_REPORT`, the forms are in `.github/ISSUE_TEMPLATE/`; Help › Get help and support opens tintaoffice.com/#contact). |

Rules: upstream first (fixes useful to everyone go to storytold as PRs); every change ships with
automated tests and updated docs in the same PR; files people exchange (.docx, .xlsx, .pptx, PDF)
come first; Spain and Ukraine first (UI language, spelling, number/date formats, national
certificates).
