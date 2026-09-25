# Issue #104 candidate review

- Baseline: `bcd2959c517409205370af2f005490965720f00d`.
- Initial candidate: `0cbf148700222aac9df8a81e7b9fc432ecd4a5ef`.
- Initial diff SHA-256: `f66950b6153e5411bd72cce860540b202302d87bbeee21bf6d765cbc49cd3dba`.
- Related receipts: `npm run test:contract` exit 0; `npx playwright test tests/governance.spec.mjs` 29 passed; design receipt validator exit 0; `git diff --check` exit 0.

## Fresh two-axis review

The reviewer compared #241–#244, server route contracts, source and changed tests. It proposed:

| ID | Axis | Proposed severity | Observable risk | Disposition |
|---|---|---|---|---|
| ISSUE104-01 | SPEC | HIGH | An edited inference response retained a previous `Safe to emit` verdict. | Independently reproduced; fixed in delta. |
| ISSUE104-02 | STANDARDS | MEDIUM | A late DP window answer could replace another tenant's current data. | Guarded by access identity and data revision in delta. |
| ISSUE104-03 | SPEC | MEDIUM | TTL history offset stayed on page two after tenant change. | Reset to page one on revision in delta. |

Independent reproduction of ISSUE104-01: `/root/repro_issue104_high` ran one temporary Playwright browser path on the initial candidate. It observed the stale safe badge after adding an API key to the textarea, with one POST request. SUT reached; workflow impact; no server emission or data leak observed. Verdict `CONFIRMED`. Test artifact `/tmp/issue-104-stale-inference-repro.spec.mjs` SHA-256 `644da0753ba040fb41c8f1952b8ddf4c9833aa1093f3331c55bbbd98ef43974d`; receipt `/tmp/issue-104-stale-inference-repro.receipt.txt` SHA-256 `b6e243c453591b9f396c6aaa65ea2d789007b795629e1e5de129e2791a953934`.

Delta verification: `npm run test:contract` exit 0, 292 en/ko keys equal; targeted Playwright test for all three reviewed paths: 3 passed. The full 29-test suite was already green on the initial candidate and was not repeated on unchanged paths.
