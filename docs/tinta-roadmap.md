# Tinta PDF — Tinta Office roadmap

> **Status:** 2026-10-27 · approved by the owner (Denys) · **Revision history:** 2026-10-27 created; 2026-10-27 synced with upstream (#13), Help › Report a problem and issue forms added.

Tinta PDF is the Tinta Office build of PdfCraft and replaces Acrobat Pro. Upstream keeps the
feature-parity roadmap ([ROADMAP.md](../ROADMAP.md), [roadmap.md](roadmap.md),
[gaps.md](gaps.md)); this page lists only what the fork does and which upstream gaps matter most
for users in Spain and Ukraine. It is a fork-only file, so upstream syncs never conflict on it.

## Where it stands (2026-10-27)

Ready for real work ≈ 50% (essentials user ≈ 66%, upstream estimate); 203 commits behind
upstream. Viewing, comments, pages, forms and protection are at 65–70%. Missing for Spain: OCR
reads only unaccented Latin (no ñ, á or Cyrillic), and there is no DNIe / smart-card signing.

## Priorities

| # | Task | Where | Stage |
|---|---|---|---|
| 1 | Signing with an FNMT certificate from the Windows store: test with a real certificate that Acrobat Reader, AutoFirma and VALIDe accept the signature; add a timestamp (TSA) | upstream + our test | 1 |
| 2 | OCR in Spanish, Catalan and Ukrainian (upstream gap 7; `ocr::LANGUAGES` is `en` only) | upstream | 1–2 |
| 3 | Government forms: a set of public AEAT, Seguridad Social and DGT forms — fill, save, print; bugs found go upstream | our tests, fixes upstream | 1 |
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
| Linux | 1 | AppImage + .deb with Tinta names for all five (built in the cloud); Flatpak in stage 2. |
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
