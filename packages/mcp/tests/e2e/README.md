# MCP ⇄ editor browser e2e

Drives a real Chrome (via `playwright-core`, `channel: 'chrome'` — no browser
download) against the live dev stack while a raw streamable-HTTP MCP client
exercises the tool surface. Covers the "current work" UI (action label,
auto-follow/spotlight, activity feed, per-tab persisted toggles) and the full
MCP smoke: validate → build (bare kinds) → automate → bounce → arrange →
verify → export, plus patch export/import and parameter sweeps.

```sh
task mcp-dev          # terminal 1: editor (:9170) + MCP server (:9171)
npm install           # once, in this directory
npm run e2e           # terminal 2
```

Requires Google Chrome installed (uses the system Chrome, headless).
Screenshots land in `./shots/`.
