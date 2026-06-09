# MCP server — status & morning checklist

Status of the MCP work described in `docs/plans/MCP.md`. Branch: **`mcp-server`**.

_All seven phases are landed, tested, and committed — **and the live
browser/MCP-client round-trip was verified end-to-end on 2026-06-09** (see "Live
verification — DONE" below). The morning checklist is kept as a reusable runbook._

## Where things stand

| Phase | What | State |
|---|---|---|
| 1 | Extract `awsm-audio-editor-protocol` crate | ✅ done, committed |
| 2 | Native `awsm-audio-mcp-server` crate | ✅ done, committed |
| 3 | Editor `remote.rs` (WebTransport client + render path) | ✅ done, committed |
| 4 | Editor connect UX (top-bar button + modal + `?mcp=`) | ✅ done, committed |
| 5 | Taskfile + config wiring | ✅ done, committed |
| 6 | Worklet authoring over MCP (`attach_wasm`) | ✅ done, committed |
| 7 | Live verification | ✅ **verified 2026-06-09** |

Commits (on `mcp-server`): Phase 1 → 6 are one commit each, plus this doc.

## Live verification — DONE (2026-06-09, Chrome)

The full browser/MCP-client round-trip was run by hand and passed:

- **Attach:** editor auto-connected via `?mcp=`; server logged `editor attached`;
  attach probe `Ok`. Top-bar button showed "MCP ✓".
- **`/debug` seam:** `Play`/`Stop` → `"Ok"`; `Query samples` → the root "main"
  Sound; `RenderWav` → a valid 16-bit stereo 48 kHz WAV on disk.
- **MCP client over `/mcp`** (raw streamable-HTTP handshake): `initialize` →
  session id + server instructions; `tools/list` → all 21 tools; `add_node`
  (oscillator) → `ok`, and the follow-up `get_snapshot` reflected the new node.
- **WAV readbacks:** with a 440 Hz oscillator auditioning, `wav_stats` →
  `peak 1.0, rms 0.707` (textbook unit sine), `waveform` → flat ±1.0 envelope.
- **Worklet authoring:** `add_node` an `audio_worklet`; built
  `awsm-audio-worklet-gain` to wasm; `attach_wasm { node, wasm_path }` →
  `get_snapshot` showed the discovered `gain` param (range 0.0–2.0); a
  deliberately-broken module returned the real `WebAssembly.compile` error.
- **Connect UX:** the top-bar button + modal disconnect/reconnect verified (server
  log shows `editor attached` → `client disconnect` → `editor attached`).
- **Gotcha found:** hand-written `/debug` JSON for a struct-variant query needs an
  explicit `"args"` (e.g. `{"Query":{"query":"wav_stats","args":{}}}`); the MCP
  tools build these in Rust so they're unaffected. The saved WAV lands in the OS
  temp dir (`std::env::temp_dir()`), which on macOS is `/var/folders/.../T/`, not
  `/tmp`.

## Natively covered (unattended — green at every commit)

- **`task lint`** (`cargo fmt --all -- --check` + `cargo clippy --all
  --all-features --tests -D warnings`) — green across the whole workspace
  (type-checks the wasm crates for the host target + all tests).
  - ⚠️ A pre-existing `vec_init_then_push` in `packages/crates/schema/src/tests.rs`
    fires only on newer-than-CI clippy (local clippy 0.1.95, no toolchain pin);
    silenced with a scoped `#[allow]` so the authoritative gate runs clean. No
    behavior change — CI's pinned toolchain never triggered it.
- **`cargo test -p awsm-audio-editor-protocol`** — 11 tests: serde round-trips
  (JSON + TOML, incl. `AttachWasm`, `Request`/`Response`/`QueryResult`), a pinned
  `Request` wire-shape test, and the pure WAV-math (`WavStats::from_pcm` /
  `WaveformEnvelope::from_pcm`: unit sine → peak≈1 / rms≈0.707, ramp envelope
  monotonic, bucket bounds).
- **`cargo test -p awsm-audio-mcp-server`** — the cert test (`GeneratedCert::new`
  + 32-byte base64url hash).
- **`cargo build -p awsm-audio-editor --target wasm32-unknown-unknown`** — the
  editor compiles to wasm (remote.rs + connect UI + the `render_pcm` /
  `attach_wasm_bytes_async` controller methods).
- **`cargo build -p awsm-audio-worklet-gain --target wasm32-unknown-unknown
  --release`** — the example Gain worklet builds to a valid ~4 KB `.wasm` (magic
  `\0asm`), proving the author→compile half of the worklet pipeline.
- **Headless server boot + `GET /control`** (no editor, no browser):
  ```
  task mcp:serve &              # or: cargo run -p awsm-audio-mcp-server -- --http-port 9171 --quic-port 9172
  sleep 2
  curl -s http://127.0.0.1:9171/control
  # → {"cert_hash":"…","quic_url":"https://127.0.0.1:9172"}
  ```
  Verified: logs the cert hash + "WebTransport (QUIC) listening on udp/9172",
  `/control` returns the URL + hash.

## Browser-only surface (now verified — see "Live verification — DONE" above)

Everything below the `/control` boot needs a hand-attached **Chrome** tab
(`serverCertificateHashes` is Chrome-only) + an MCP client, so it can't run
unattended — but it was all exercised live on 2026-06-09 and passed. The morning
checklist below remains the reusable runbook for re-verifying after changes.

## Known follow-ups (not blocking)

- **Phase 3.5 (push events):** `remote::notify_event` is implemented but not yet
  called from the controller's toast/selection emitters, so the agent doesn't get
  live `EditorEvent` notifications yet (the request/response path is unaffected).
  It's `#[allow(dead_code)]` until wired. Wire it where the controller sets
  `status` / changes selection: emit `EditorEvent { kind: "toast", level, message }`
  and `{ kind: "selection", nodes }`.
- **Typed MCP tools:** the server ships discovery + the WAV readbacks + transport
  + a few ergonomic mutators (`add_node`/`connect`/`set_field`/`remove_node`/
  `bounce`/`set_root`) + `attach_wasm` + the generic escape hatches
  (`dispatch_command`/`dispatch_batch`/`run_query`). The remaining ~20 typed
  wrappers from spec §2.6 (per-`SongOp`/`ControlOp`/`ArrangeOp` etc.) are
  reachable today via `dispatch_command`; add ergonomic wrappers as desired.

---

## Morning checklist (run by a human, in order)

1. **Start the server:** `task mcp:serve` → logs the cert hash and
   "WebTransport (QUIC) listening on udp/9172".
2. **Start the editor:** `task editor:dev` → serves on `:9170`.
   (Or `task mcp-dev` to run both at once.)
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
   `awsm-audio-worklet` (the `awsm://docs/worklet-abi` resource has the recipe;
   `packages/worklets/gain` is the worked example); `cargo build -p <crate>
   --target wasm32-unknown-unknown --release`; call `attach_wasm { node,
   wasm_path }`; confirm `get_snapshot` shows the discovered `gain` param, then
   wire it and `render_wav` to hear it. Also confirm a deliberately-broken module
   returns the compile/ABI error.

If any step fails, the fix is almost always in `remote.rs` (`dispatch`/framing)
or a serde-shape mismatch — add/adjust a §1.6 round-trip test to pin it, then
re-verify.
