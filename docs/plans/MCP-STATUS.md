# MCP server — status & morning checklist

Status of the MCP work described in `docs/plans/MCP.md`. Branch: **`mcp-server`**.

_Last updated after Phase 2._

## Where things stand

| Phase | What | State |
|---|---|---|
| 1 | Extract `awsm-audio-editor-protocol` crate | ✅ done, committed |
| 2 | Native `awsm-audio-mcp-server` crate | ✅ done, committed |
| 3 | Editor `remote.rs` (WebTransport client + dispatch) | ⏳ in progress / next |
| 4 | Editor connect UX (button + modal + `?mcp=`) | ⏳ pending |
| 5 | Taskfile + config wiring | ⏳ pending |
| 6 | Worklet authoring over MCP (`attach_wasm`) | ⏳ pending |
| 7 | Live verification | DEFERRED (this checklist) |

## Natively covered (unattended, green at each commit)

- **`task lint`** (`cargo fmt --all -- --check` + `cargo clippy --all
  --all-features --tests -D warnings`) passes across the whole workspace.
  - ⚠️ A pre-existing `vec_init_then_push` in `packages/crates/schema/src/tests.rs`
    fires only on newer-than-CI clippy (local clippy 0.1.95); silenced with a
    scoped `#[allow]` so the authoritative gate runs clean. No behavior change.
- **`cargo test -p awsm-audio-editor-protocol`** — 7 serde round-trip tests
  (JSON + TOML, incl. `AttachWasm`, `Request`/`Response`/`QueryResult`,
  `WavStats`/`Waveform`, and a pinned `Request` wire-shape test).
- **`cargo test -p awsm-audio-mcp-server`** — the cert test (`GeneratedCert::new`
  + 32-byte base64url hash).
- **`cargo build -p awsm-audio-editor --target wasm32-unknown-unknown`** — the
  editor still compiles to wasm after the Phase 1 refactor.
- **Headless server boot + `GET /control`** (no editor, no browser):
  ```
  cargo run -p awsm-audio-mcp-server -- --http-port 9171 --quic-port 9172 &
  sleep 2
  curl -s http://127.0.0.1:9171/control
  # → {"cert_hash":"…","quic_url":"https://127.0.0.1:9172"}
  kill %1
  ```
  Verified: logs the cert hash + "WebTransport (QUIC) listening on udp/9172",
  `/control` returns the URL + hash.

## Browser / MCP-client deferred (needs a hand-attached Chrome tab)

The live WebTransport round-trip (`serverCertificateHashes` is Chrome-only) and
the MCP-client tool calls can't run unattended — see the morning checklist below.

## Exact next step

Phase 3: port the editor's `remote.rs` (WebTransport client + the sync `dispatch`
interpreter + the async render path) from
`../awsm-renderer/packages/frontend/editor/src/remote.rs`, add `render_pcm` to the
controller, and keep the pure WAV-math helpers (`compute_wav_stats` /
`compute_waveform`) native-testable.

---

## Morning checklist (run by a human, in order)

1. **Start the server:** `task mcp:serve` → logs the cert hash and
   "WebTransport (QUIC) listening on udp/9172".
2. **Start the editor:** `task editor:dev` → serves on `:9170`.
3. **Attach:** open `http://localhost:9170/?mcp=http://127.0.0.1:9171` in
   **Chrome**. Server logs "editor attached"; the top-bar MCP button shows
   connected.
4. **Raw round-trip via `/debug`** (server up, editor attached):
   ```
   curl -s -X POST http://127.0.0.1:9171/debug \
     -H 'content-type: application/json' -d '{"Play":null}'        # → "Ok"
   curl -s -X POST http://127.0.0.1:9171/debug \
     -H 'content-type: application/json' -d '{"Query":{"query":"samples"}}'
   ```
   → `Ok`, then the sample list as JSON. (The `Request` enum is externally
   tagged; the inner `EditorQuery` uses `tag:"query"`. The exact JSON shapes are
   pinned by the §1.6 serde tests — copy a payload from a test if unsure.)
5. **WAV render:**
   ```
   curl -s -X POST http://127.0.0.1:9171/debug \
     -H 'content-type: application/json' -d '{"RenderWav":{}}'
   ```
   → `{ "Wav": { "bytes": N, "saved": true, "path": "/tmp/awsm-audio-mcp-last.wav" } }`.
   Play `/tmp/awsm-audio-mcp-last.wav` to confirm it's the root Sound.
6. **MCP client:** point an MCP client (e.g. Claude Code) at
   `http://127.0.0.1:9171/mcp` (streamable HTTP). Confirm `get_snapshot`,
   `list_samples`, `render_wav`, `wav_stats`, `waveform`, and a mutation
   (`add_node`) all work, and that a follow-up `get_snapshot` reflects the
   mutation.
7. **Connect UX:** confirm the top-bar button + modal connect/disconnect, and
   that loading without `?mcp=` and connecting via the modal also works.
8. **Worklet authoring (Phase 6):** via the MCP client, `add_node` an
   `audio_worklet` node; author a trivial `Gain` crate against
   `awsm-audio-worklet`; `cargo build -p <crate> --target wasm32-unknown-unknown
   --release`; call `attach_wasm { node, wasm_path }`; confirm `get_snapshot`
   shows the discovered `gain` param, then wire it and `render_wav` to hear it.
   Also confirm a deliberately-broken module returns the compile/ABI error.

If any step fails, the fix is almost always in `remote.rs` (`dispatch`/framing)
or a serde-shape mismatch — add/adjust a §1.6 round-trip test to pin it, then
re-verify.
