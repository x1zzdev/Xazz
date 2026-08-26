# Xazz product UX prototype

Local-only React prototype for the approved landing-to-tool experience.

## Run

```powershell
npm install
npx playwright install chromium
npm run dev
```

Open `http://127.0.0.1:5173`.

## Verify

```powershell
npm run build
npm run test:contract
npm run test:contrast
npm run test:e2e
npm audit --audit-level=high
```

`npm run capture` refreshes the nine repository screenshots after a source
change. Do not use it as user-testing evidence.

## Scope and truth boundary

- Synthetic fixture only: 100 input rows → 41 output rows.
- No production backend, authentication, upload, or deployment.
- Live Check is a Demo with a Future backend contract.
- Policy, DP, sLM repair, sandboxing, Burn training, monitoring, and durable
  audit remain Research or Planned.
- A process exit is not promoted to pipeline success without structured result
  evidence.
- The successful fixture requests no artifact. Browser CSV export is an
  optional post-result user action.

Figma handoff:
<https://www.figma.com/design/WqomiXUlVze79yz3s0GRmS>
