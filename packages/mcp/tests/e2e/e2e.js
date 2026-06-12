// E2E verification of the MCP "current work" surfaces:
//  - live action label on the 🤖 chip
//  - auto-follow / spotlight (incl. opening the arranger)
//  - activity feed panel
//  - per-tab persisted toggles (localStorage keyed by sessionStorage tab id)
//
// Drives a real Chrome via playwright-core and the MCP server via raw
// streamable-HTTP JSON-RPC. Editor: http://127.0.0.1:9170, MCP: 127.0.0.1:9171.

const { chromium } = require('playwright-core');

const EDITOR = 'http://127.0.0.1:9170';
const MCP = 'http://127.0.0.1:9171';
const SHOTS = require('path').join(__dirname, 'shots');

// ---- minimal MCP streamable-HTTP client -----------------------------------
class Mcp {
  constructor(base) { this.base = base; this.sid = null; this.next = 1; }
  headers() {
    const h = { 'content-type': 'application/json', accept: 'application/json, text/event-stream' };
    if (this.sid) h['mcp-session-id'] = this.sid;
    return h;
  }
  async rpc(method, params) {
    const id = this.next++;
    const res = await fetch(this.base + '/mcp', {
      method: 'POST', headers: this.headers(),
      body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
    });
    if (!res.ok) throw new Error(`${method}: HTTP ${res.status} ${await res.text()}`);
    const sid = res.headers.get('mcp-session-id');
    if (sid) this.sid = sid;
    const ct = res.headers.get('content-type') || '';
    if (!ct.includes('event-stream')) {
      const t = await res.text();
      return t ? JSON.parse(t) : null;
    }
    // Read the SSE stream until the response with our id appears.
    const reader = res.body.getReader();
    const dec = new TextDecoder();
    let buf = '';
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      for (const line of buf.split('\n')) {
        if (!line.startsWith('data:')) continue;
        const payload = line.slice(5).trim();
        if (!payload) continue;
        try {
          const msg = JSON.parse(payload);
          if (msg.id === id) { reader.cancel().catch(() => {}); return msg; }
        } catch { /* partial line; keep buffering */ }
      }
    }
    throw new Error(`${method}: SSE ended without response`);
  }
  async notify(method, params) {
    await fetch(this.base + '/mcp', {
      method: 'POST', headers: this.headers(),
      body: JSON.stringify({ jsonrpc: '2.0', method, params }),
    });
  }
  async init() {
    const r = await this.rpc('initialize', {
      protocolVersion: '2025-03-26', capabilities: {},
      clientInfo: { name: 'e2e', version: '0' },
    });
    if (r.error) throw new Error('initialize: ' + JSON.stringify(r.error));
    await this.notify('notifications/initialized', {});
    return r.result;
  }
  async tool(name, args = {}) {
    const r = await this.rpc('tools/call', { name, arguments: args });
    if (r.error) throw new Error(`${name}: ${JSON.stringify(r.error)}`);
    if (r.result?.isError) throw new Error(`${name}: tool error: ${JSON.stringify(r.result.content)}`);
    return r.result;
  }
  toolText(result) {
    return (result.content || []).map((c) => c.text || '').join('\n');
  }
  async close() {
    if (this.sid) await fetch(this.base + '/mcp', { method: 'DELETE', headers: { 'mcp-session-id': this.sid } }).catch(() => {});
  }
}

// ---- assertion helpers ------------------------------------------------------
let passed = 0, failed = 0;
function ok(cond, label, extra = '') {
  if (cond) { passed++; console.log(`  PASS  ${label}`); }
  else { failed++; console.log(`  FAIL  ${label}${extra ? ' — ' + extra : ''}`); }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Poll `fn` (in page) every `step` ms up to `timeout`; resolve true on first truthy.
async function pollPage(page, fn, timeout = 3000, step = 40) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await page.evaluate(fn)) return true;
    await sleep(step);
  }
  return false;
}

// The 🤖 chip's visible text ("idle" / "working…" / a live action label).
const chipText = `(() => {
  const el = document.querySelector('[title^="Agent"]');
  return el ? el.textContent : null;
})()`;

// All feed-panel rows' text (the panel header contains "Agent activity").
const feedRows = `(() => {
  const span = [...document.querySelectorAll('span')].find(s => s.textContent === 'Agent activity');
  if (!span) return null;
  const panel = span.closest('div').parentElement; // header row → panel
  return [...panel.children].slice(1).map(d => d.textContent);
})()`;

async function openModal(page) {
  await page.click('button:has-text("MCP")');
  await page.waitForSelector('h2:has-text("Connect to MCP server")');
}
async function closeModal(page) {
  await page.click('button:has-text("Close")');
  await page.waitForSelector('h2:has-text("Connect to MCP server")', { state: 'detached' });
}
function toggleBox(page, label) {
  return page.locator('label', { hasText: label }).locator('input[type=checkbox]');
}

(async () => {
  const fs = require('fs');
  fs.mkdirSync(SHOTS, { recursive: true });
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => console.log('  [pageerror]', e.message));

  console.log('— Phase A: connect + defaults');
  await page.goto(`${EDITOR}/?mcp=127.0.0.1:9171`);
  await page.waitForSelector('button:has-text("MCP ✓")', { timeout: 20000 });
  ok(true, 'editor connected to MCP (MCP ✓)');
  ok((await page.evaluate(chipText))?.includes('idle'), 'chip reads idle when at rest');

  await openModal(page);
  ok(await toggleBox(page, 'Show the current action').isChecked(), 'default: action label ON');
  ok(await toggleBox(page, 'Follow the agent').isChecked(), 'default: auto-follow ON');
  ok(!(await toggleBox(page, 'Show the activity feed').isChecked()), 'default: feed OFF');
  await toggleBox(page, 'Show the activity feed').click();
  ok(await toggleBox(page, 'Show the activity feed').isChecked(), 'feed toggle turns ON');
  await closeModal(page);

  console.log('— Phase B: agent drives; label + spotlight + feed');
  const mcp = new Mcp(MCP);
  globalThis.__mcp = mcp;
  await mcp.init();

  // pairing_status works pre-pairing and reports an attachable editor.
  const pairing = JSON.parse(mcp.toolText(await mcp.tool('pairing_status')));
  ok(typeof pairing.pair_code === 'string' && pairing.editors_connected === 1,
     'pairing_status reports 1 editor + a pair code', JSON.stringify(pairing));
  if (!pairing.paired && !pairing.status.startsWith('ready')) {
    // A stale agent session (e.g. an earlier crashed run) makes auto-bind
    // ambiguous — pair explicitly through the modal, which is itself a test.
    await openModal(page);
    await page.fill('input[placeholder*="pairing code"]', pairing.pair_code);
    await page.press('input[placeholder*="pairing code"]', 'Enter');
    await sleep(300);
    const after = JSON.parse(mcp.toolText(await mcp.tool('pairing_status')));
    ok(after.paired, 'explicit pairing via the modal succeeded', JSON.stringify(after));
  }

  // add_node: watch for the live label + the spotlight glow while serving.
  const labelSeen = pollPage(page, `(() => { const t = ${chipText}; return t && t.includes('Adding'); })()`, 2500, 25);
  const glowSeen = pollPage(page, `(() => [...document.querySelectorAll('.node')].some(n => (n.style.animation||'').includes('mcp-spotlight')))()`, 2500, 25);
  // BARE string kind — the exact case that used to die with
  // `expected value at line 1 column 1`.
  await mcp.tool('add_node', { kind: 'oscillator', x: 200, y: 160 });
  ok(await labelSeen, 'chip showed "Adding Oscillator" while serving');
  ok(await glowSeen, 'new node flashed the spotlight glow');
  await page.screenshot({ path: `${SHOTS}/1-add-node.png` });

  await mcp.tool('add_node', { kind: 'gain', x: 460, y: 160 });
  // Feed retains entries — assert both adds are logged (newest first).
  await sleep(200);
  const rows = await page.evaluate(feedRows);
  ok(Array.isArray(rows) && rows.some((r) => r.includes('Adding Oscillator')), 'feed logged "Adding Oscillator"', JSON.stringify(rows));
  ok(Array.isArray(rows) && rows.some((r) => r.includes('Adding Gain')), 'feed logged "Adding Gain"', JSON.stringify(rows));
  await page.screenshot({ path: `${SHOTS}/2-feed.png` });

  console.log('— Phase C: auto-follow opens the arranger');
  // Create an arrangement WITH bpm + named tracks in one call (the new batch
  // form), then edit it — the follow logic must flip the body to the arranger.
  const created = JSON.parse(mcp.toolText(
    await mcp.tool('create_arrangement', { bpm: 120, length_secs: 16, tracks: ['Drums', 'Bass'] })));
  ok(created.ok === true && created.tracks.length === 2, 'create_arrangement with bpm/length/tracks', JSON.stringify(created));
  await mcp.tool('add_arrangement_track');
  const arrangerOpen = await pollPage(page, `(() => !!document.querySelector('span') && [...document.querySelectorAll('span')].some(s => s.textContent === 'Add Track'))()`, 3000);
  ok(arrangerOpen, 'arranger view opened when the agent edited the arrangement');
  await page.screenshot({ path: `${SHOTS}/3-arranger.png` });

  // Switch back to the sound → canvas (Sounds view) follows.
  const samples = JSON.parse(mcp.toolText(await mcp.tool('list_samples')));
  const soundId = (samples.data || samples).find?.((s) => s.kind === 'sound')?.id
    ?? (samples.data || []).find((s) => s.kind === 'sound')?.id;
  ok(!!soundId, 'list_samples returned the sound id');
  if (soundId) {
    await mcp.tool('set_active_sample', { sample: soundId });
    const soundsBack = await pollPage(page, `(() => ![...document.querySelectorAll('span')].some(s => s.textContent === 'Add Track'))()`, 3000);
    ok(soundsBack, 'view followed back to the Sounds canvas');
  }

  console.log('— Phase E: full MCP smoke (validate → build → automate → bounce → arrange → verify → export)');
  // Dry-run the batch first (validate_command), then build for real with
  // dispatch_refs using BARE kind tags inside add_node args.
  const batch = [
    { cmd: 'add_node', ref: 'osc', args: { kind: 'oscillator', x: 100, y: 400 } },
    { cmd: 'add_node', ref: 'amp', args: { kind: 'gain', x: 300, y: 400 } },
    { cmd: 'add_node', ref: 'out', args: { kind: 'output', x: 500, y: 400 } },
    { cmd: 'connect', args: { from: '$osc', to: '$amp' } },
    { cmd: 'connect', args: { from: '$amp', to: '$out' } },
  ];
  const validation = JSON.parse(mcp.toolText(await mcp.tool('validate_command', { commands: batch })));
  ok(validation.results.every((r) => r.ok), 'validate_command dry-run passes the batch', JSON.stringify(validation));
  const built = JSON.parse(mcp.toolText(await mcp.tool('dispatch_refs', { commands: batch })));
  ok(!!built.refs.osc && !!built.refs.amp && !!built.refs.out, 'dispatch_refs built the chain with bare kinds', JSON.stringify(built));

  // Typed set_automation (the worst schema-exploration loop in the field notes).
  await mcp.tool('set_automation', {
    node: built.refs.amp, param: 'gain',
    events: [
      { event: 'set_value', args: { value: 0.0, time: 0.0 } },
      { event: 'linear_ramp', args: { value: 0.8, time: 0.05 } },
      { event: 'exponential_ramp', args: { value: 0.001, time: 0.9 } },
    ],
  });
  ok(true, 'set_automation typed tool accepted an envelope');

  // Typed add_boundary (was: `command 2: missing field port`).
  const boundary = JSON.parse(mcp.toolText(await mcp.tool('add_boundary', { port: 'outlet', x: 700, y: 400 })));
  ok(boundary.ok === true && !!boundary.id, 'add_boundary typed tool returns the minted id', JSON.stringify(boundary));

  // Bounce with an explicit duration: the stored bounce must be exactly that
  // span (the 0.05s loop-fold truncation bug). Output-node graph, no sequencer.
  const bounced = JSON.parse(mcp.toolText(await mcp.tool('bounce', { sample: soundId, duration_secs: 1.0 })));
  ok(Math.abs(bounced.stored_duration_secs - 1.0) < 0.02,
     'bounce duration_secs is literal: stored ≈ 1.0s (was 0.05s pre-fix)', JSON.stringify(bounced));
  ok(bounced.rendered_duration_secs !== undefined && bounced.stored_duration_secs !== undefined,
     'bounce reports rendered vs stored durations');

  // Place the bounce in the arrangement, verify, export.
  const arrId = created.id;
  await mcp.tool('set_active_sample', { sample: arrId });
  await mcp.tool('add_clip', { track: 0, start: 0, source: soundId });
  const verdict = JSON.parse(mcp.toolText(await mcp.tool('verify_arrangement')));
  ok(verdict.master && typeof verdict.master.peak === 'number' && Array.isArray(verdict.recommendations),
     'verify_arrangement returns master stats + recommendations', JSON.stringify(verdict).slice(0, 300));
  ok(typeof verdict.master.crest_factor === 'number', 'wav stats carry the new transient readbacks');

  const exported = JSON.parse(mcp.toolText(
    await mcp.tool('export_wav', { path: require('path').join(__dirname, 'shots') + '/', sample: arrId })));
  const fileOk = require('fs').existsSync(exported.path) && require('fs').statSync(exported.path).size > 100;
  ok(exported.ok === true && fileOk, `export_wav wrote ${exported.path}`, JSON.stringify(exported));

  console.log('— Phase F: experiment tools (notes, sweep, sections, trim, patch export/import)');
  // Working notes show up in list_samples.
  await mcp.tool('set_sample_notes', { sample: soundId, notes: 'keeper — tight envelope' });
  const samples2 = JSON.parse(mcp.toolText(await mcp.tool('list_samples')));
  const annotated = (samples2.data || []).find((s) => s.id === soundId);
  ok(annotated?.notes?.includes('keeper'), 'set_sample_notes round-trips through list_samples', JSON.stringify(annotated));

  // The graph-wide modulation map names the gain param on the amp node.
  await mcp.tool('set_active_sample', { sample: soundId });
  const mods = JSON.parse(mcp.toolText(await mcp.tool('list_modulation_targets')));
  const ampTarget = (mods.data || []).find((t) => t.node === built.refs.amp);
  ok(ampTarget?.params?.includes('gain'), 'list_modulation_targets maps the amp gain', JSON.stringify(mods).slice(0, 200));

  // A two-point sweep measures and then restores the original value.
  const sweep = JSON.parse(mcp.toolText(await mcp.tool('parameter_sweep', {
    node: built.refs.amp, key: 'gain', values: [0.2, 0.9], duration_secs: 0.3,
  })));
  ok(sweep.points?.length === 2 && sweep.points[0].stats && sweep.restored_to !== undefined,
     'parameter_sweep returns per-value stats and restores', JSON.stringify(sweep).slice(0, 200));

  // Named sections persist on the arrangement.
  await mcp.tool('set_active_sample', { sample: arrId });
  await mcp.tool('set_arrangement_sections', { sections: [
    { name: 'intro', start: 0, end: 4 }, { name: 'main', start: 4, end: 16 },
  ] });
  const arrText = mcp.toolText(await mcp.tool('get_arrangement'));
  ok(arrText.includes('intro') && arrText.includes('main'), 'set_arrangement_sections round-trips via get_arrangement');

  // trim_silence: build a dedicated decaying blip (its gain ramps below the
  // -60 dB floor by ~0.3s), render a 1.0s window, and expect the trimmed
  // export to be much shorter. (A fresh sound — the main one has stray
  // unconnected nodes that audition at full level and would mask the decay.)
  const blip = JSON.parse(mcp.toolText(await mcp.tool('dispatch_command', {
    command: { cmd: 'add_sample', args: { kind: 'sound' } },
  })));
  const blipBuilt = JSON.parse(mcp.toolText(await mcp.tool('dispatch_refs', { commands: [
    { cmd: 'add_node', ref: 'osc', args: { kind: 'oscillator', x: 100, y: 100 } },
    { cmd: 'add_node', ref: 'amp', args: { kind: 'gain', x: 300, y: 100 } },
    { cmd: 'add_node', ref: 'out', args: { kind: 'output', x: 500, y: 100 } },
    { cmd: 'connect', args: { from: '$osc', to: '$amp' } },
    { cmd: 'connect', args: { from: '$amp', to: '$out' } },
  ] })));
  await mcp.tool('set_automation', {
    node: blipBuilt.refs.amp, param: 'gain',
    events: [
      { event: 'set_value', args: { value: 1.0, time: 0.0 } },
      { event: 'exponential_ramp', args: { value: 0.0005, time: 0.3 } },
    ],
  });
  const trimmed = JSON.parse(mcp.toolText(await mcp.tool('export_wav', {
    path: require('path').join(__dirname, 'shots') + '/trimmed.wav',
    sample: blip.id, duration_secs: 1.0, trim_silence: true,
  })));
  ok(trimmed.duration_secs < 0.6, `trim_silence shortened the export (${trimmed.duration_secs}s ≪ 1.0s)`, JSON.stringify(trimmed));

  // Patch round-trip: export the sound, delete it, import it back.
  const patch = JSON.parse(mcp.toolText(await mcp.tool('export_sample', {
    sample: soundId, path: require('path').join(__dirname, 'shots') + '/patch.toml',
  })));
  ok(patch.ok === true && require('fs').existsSync(patch.path), 'export_sample wrote a patch TOML', JSON.stringify(patch));
  // Importing while it still exists must be rejected with a clear message.
  let collision = '';
  try { await mcp.tool('import_sample', { path: patch.path }); } catch (e) { collision = String(e); }
  ok(collision.includes('already exists'), 'import_sample rejects an id collision clearly', collision.slice(0, 160));
  await mcp.tool('dispatch_command', { command: { cmd: 'remove_sample', args: { id: soundId } } });
  const imported = JSON.parse(mcp.toolText(await mcp.tool('import_sample', { path: patch.path })));
  ok(imported.ok === true && imported.imported.some((s) => s.id === soundId),
     'import_sample restored the deleted patch (same id)', JSON.stringify(imported));

  console.log('— Phase D: per-tab persistence');
  const id1 = await page.evaluate(`sessionStorage.getItem('awsm.tab_id')`);
  ok(!!id1, 'tab 1 minted a session tab id');
  const k1 = await page.evaluate(`localStorage.getItem('awsm.mcp.show_feed.' + sessionStorage.getItem('awsm.tab_id'))`);
  ok(k1 === '1', 'tab 1 wrote its per-tab feed key (=1)');

  await page.reload();
  await page.waitForSelector('button:has-text("MCP")', { timeout: 20000 });
  const id1b = await page.evaluate(`sessionStorage.getItem('awsm.tab_id')`);
  ok(id1b === id1, 'tab id survives reload');
  await openModal(page);
  ok(await toggleBox(page, 'Show the activity feed').isChecked(), 'feed toggle persisted across reload');
  await closeModal(page);

  // Second tab (same context = same localStorage, fresh sessionStorage).
  const page2 = await ctx.newPage();
  await page2.goto(`${EDITOR}/`); // no ?mcp → never contends for the agent binding
  await page2.waitForSelector('button:has-text("MCP")', { timeout: 20000 });
  const id2 = await page2.evaluate(`sessionStorage.getItem('awsm.tab_id')`);
  ok(!!id2 && id2 !== id1, 'tab 2 minted a different tab id');
  await openModal(page2);
  ok(await toggleBox(page2, 'Show the activity feed').isChecked(), 'tab 2 inherited the feed=ON seed');
  await toggleBox(page2, 'Show the activity feed').click(); // OFF in tab 2
  await closeModal(page2);
  const k2 = await page2.evaluate(`localStorage.getItem('awsm.mcp.show_feed.' + sessionStorage.getItem('awsm.tab_id'))`);
  ok(k2 === '0', 'tab 2 wrote its own per-tab key (=0)');

  // Tab 1 must be unaffected by tab 2's change.
  await page.reload();
  await page.waitForSelector('button:has-text("MCP")', { timeout: 20000 });
  await openModal(page);
  ok(await toggleBox(page, 'Show the activity feed').isChecked(), "tab 2's change did NOT stomp tab 1 (feed still ON)");
  await closeModal(page);

  console.log(`\n${passed} passed, ${failed} failed`);
  await mcp.close();
  await browser.close();
  process.exit(failed ? 1 : 0);
})().catch(async (e) => {
  console.error('FATAL', e);
  // Free the agent session so a rerun can auto-pair.
  try { if (globalThis.__mcp) await globalThis.__mcp.close(); } catch {}
  process.exit(2);
});
