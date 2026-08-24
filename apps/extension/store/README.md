# Microsoft Edge Add-ons submission kit

This directory contains the material prepared for the Microsoft Edge Add-ons Partner Center. The uploadable package is produced by the extension build at `apps/extension/build/aipass-extension.zip`; the release workflow publishes a copy named `aipass-edge-extension.zip`.

For a Chinese step-by-step checklist, see [提交清单.zh-CN.md](提交清单.zh-CN.md).

## Submission order

1. Run `pnpm install --frozen-lockfile` and `node scripts/set-extension-release-version.mjs <version>`.
2. Run `pnpm --filter @aipass/extension build`.
3. Upload `apps/extension/build/aipass-extension.zip` as the package.
4. Use [listing.md](listing.md) for the English listing fields and [privacy-disclosures.json](privacy-disclosures.json) for the Privacy page.
5. Upload `assets/edge-logo-300.png`, `assets/edge-tile-small-440x280.png`, `assets/edge-tile-large-1400x560.png`, and the two PNG screenshots. The logo is required; tiles and screenshots are optional but prepared for the listing.
6. Before submitting, replace `{{EDGE_EXTENSION_ID}}` in [certification-notes.md](certification-notes.md) with the ID assigned by Partner Center and follow the native-host pairing steps.

The package is self-contained and Manifest V3. It does not load or execute remote code. The native host is a separately installed local AIPass component; the extension continues to show a disconnected state when the host is not installed.
