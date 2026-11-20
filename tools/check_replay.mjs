// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Watches replays by id in a browser and fails unless each one plays: its file comes and it starts (no message that it
// cannot be played), its time goes on, the picture is drawn, the controls do what they say, and the game's memory stays
// put while it plays. Headless and muted. No npm dependencies for Chrome; Node 22+.
//
//   node tools/check_replay.mjs BASE_URL REPLAY_ID [REPLAY_ID ...]
//
// BASE_URL is a server of the build to check (`hotdog-server --root ... --replays DIR`, which serves each replay of DIR
// at /replays/<id>). Each replay is watched twice in Chrome (the real GPU): at 1280x720 with a mouse, its keys (P pauses
// and plays, ] goes faster); and as a phone held upright (428x746 at 3x, touch, an iPhone's user agent), where the page's
// own controls (touch.js, src/replay/page.rs) must show instead of the game's buttons and answer taps: pause and play, a
// speed, the view, and a tap half way along the timeline. Then it plays at 2x for SECONDS (HOTDOG_CHECK_SECONDS, 60),
// the WebAssembly memory read every second: it fails if the memory grows by more than GROWTH megabytes after the first
// few seconds (HOTDOG_WASM_GROWTH_MB, 48), or is ever over CEILING (HOTDOG_WASM_CEILING_MB, 900: the browser build takes
// about 400 to play any match). A picture with next to no detail (black, or one flat colour) fails, as do an error the
// page did not catch and a panic.
//
// With HOTDOG_PLAYWRIGHT (the directory of a playwright-core package whose browsers are installed) the phone is also
// watched in its WebKit, the nearest to Safari on an iPhone there is off one (iPhone 14 Plus: its viewport, 3x, touch and
// user agent); WebKit there draws with the processor, slowly, so its time is only checked to go on.
//
// CHROME_PATH picks the browser (Google Chrome's own place by default). The screenshots, each run's samples and its
// console go to OUT (HOTDOG_CHECK_OUT; otherwise a new directory in HOTDOG_CHECK_TEMP or the system's). Exits 1 if a
// replay failed, 2 if it could not be checked.
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const [base, ...ids] = process.argv.slice(2);
if (!base || !ids.length) { console.error('usage: check_replay.mjs BASE_URL REPLAY_ID [REPLAY_ID ...]'); process.exit(2); }
const seconds = Number(process.env.HOTDOG_CHECK_SECONDS || 60);
const growthMb = Number(process.env.HOTDOG_WASM_GROWTH_MB || 48), ceilingMb = Number(process.env.HOTDOG_WASM_CEILING_MB || 900);
const temp = process.env.HOTDOG_CHECK_TEMP || tmpdir();
const out = process.env.HOTDOG_CHECK_OUT || await mkdtemp(join(temp, 'hotdog-replay-'));
await mkdir(out, { recursive: true });
const IPHONE = 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1';

/** A PNG's pixels (8 bits a channel, RGB or RGBA). */
function decode(png) {
  let at = 8, w = 0, h = 0, type = 0;
  const data = [];
  while (at < png.length) {
    const length = png.readUInt32BE(at), kind = png.toString('ascii', at + 4, at + 8), body = png.subarray(at + 8, at + 8 + length);
    if (kind === 'IHDR') {
      [w, h, type] = [body.readUInt32BE(0), body.readUInt32BE(4), body[9]];
      if (body[8] !== 8 || (type !== 2 && type !== 6)) throw new Error('not an 8-bit RGB or RGBA PNG');
    }
    if (kind === 'IDAT') data.push(body);
    at += 12 + length;
  }
  const bpp = type === 6 ? 4 : 3, raw = inflateSync(Buffer.concat(data)), stride = w * bpp, px = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const filter = raw[y * (stride + 1)], row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? px[y * stride + x - bpp] : 0, b = y > 0 ? px[(y - 1) * stride + x] : 0, c = x >= bpp && y > 0 ? px[(y - 1) * stride + x - bpp] : 0;
      const p = a + b - c, [pa, pb, pc] = [Math.abs(p - a), Math.abs(p - b), Math.abs(p - c)];
      const predicted = [0, a, b, (a + b) >> 1, pa <= pb && pa <= pc ? a : pb <= pc ? b : c][filter];
      px[y * stride + x] = (row[x] + predicted) & 255;
    }
  }
  return { w, h, bpp, px };
}
/** How much the picture over the replay's bar varies (as check_tab_switch.mjs measures it): the spread of the
 * brightness of squares of 8 by 8 pixels, a fiftieth left out at each end. A scene drawn spreads over 40 at almost every
 * moment; black or one colour under 15. Each run is judged by the best of its pictures. */
function detail(png) {
  const { w, h, bpp, px } = decode(png);
  const squares = [];
  for (let y = Math.floor(h * 0.15); y + 8 <= h * 0.6; y += 8) {
    for (let x = Math.floor(w * 0.1); x + 8 <= w * 0.9; x += 8) {
      let sum = 0;
      for (let dy = 0; dy < 8; dy++) for (let dx = 0; dx < 8; dx++) { const i = ((y + dy) * w + x + dx) * bpp; sum += 0.2126 * px[i] + 0.7152 * px[i + 1] + 0.0722 * px[i + 2]; }
      squares.push(sum / 64);
    }
  }
  squares.sort((a, b) => a - b);
  return squares[Math.floor(0.98 * (squares.length - 1))] - squares[Math.floor(0.02 * (squares.length - 1))];
}
const BARE = 30;

/** Kept on the page before anything of it runs: the game's WebAssembly memory (its instance's export), and the errors
 * nothing caught. */
const CAPTURE = `(()=>{const keep=r=>{try{const i=r&&(r.instance||r);const m=i&&i.exports&&i.exports.memory;if(m&&m.buffer)window.__checkMemory=m}catch{}return r};
 const is=WebAssembly.instantiateStreaming;if(is)WebAssembly.instantiateStreaming=function(...a){return is.apply(this,a).then(keep)};
 const ins=WebAssembly.instantiate;WebAssembly.instantiate=function(...a){return ins.apply(this,a).then(keep)};
 window.__checkErrors=[];addEventListener('error',e=>window.__checkErrors.push(String(e.message)));addEventListener('unhandledrejection',e=>window.__checkErrors.push(String(e.reason&&e.reason.message||e.reason)))})();`;
const SAMPLE = `({mb:window.__checkMemory?window.__checkMemory.buffer.byteLength/1048576:null,st:window.hotdogReplay||null,note:document.querySelector('#replay-note')?.hidden===false?document.querySelector('#replay-note').textContent:'',
 bar:!!document.querySelector('.touch-replay:not([hidden])'),buttons:[...document.querySelectorAll('.touch .tb')].filter(b=>!b.hidden&&b.offsetParent).length,errors:window.__checkErrors||[],line:window.hotdogMetrics||''})`;

// ----- Chrome over its DevTools protocol.
async function chrome() {
  const profile = await mkdtemp(join(temp, 'hotdog-replay-chrome-'));
  const browser = spawn(process.env.CHROME_PATH || '/opt/google/chrome/chrome', [
    '--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-extensions', '--no-first-run', '--no-default-browser-check',
    '--disable-background-networking', '--ozone-platform=headless', '--force-color-profile=srgb', '--hide-scrollbars', '--mute-audio',
    '--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist',
    '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1', '--user-data-dir=' + profile, 'about:blank',
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  let log = '';
  browser.stderr.on('data', data => { log = (log + data).slice(-20000); });
  let port, path;
  for (let attempt = 0; attempt < 100 && !port; attempt++) {
    try { [port, path] = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n'); } catch {}
    if (browser.exitCode !== null) throw new Error('Chrome exited: ' + log);
    if (!port) await sleep(200);
  }
  const socket = new WebSocket(`ws://127.0.0.1:${port}${path}`);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0, logs = [];
  const pending = new Map(), sessions = new Set();
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id) { pending.get(message.id)?.(message); pending.delete(message.id); return; }
    if (!sessions.has(message.sessionId)) return;
    if (message.method === 'Runtime.consoleAPICalled') logs.push(`${message.params.type}: ${message.params.args.map(a => a.value ?? a.description ?? '').join(' ')}`);
    if (message.method === 'Runtime.exceptionThrown') logs.push(`exception: ${message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text}`);
  };
  const call = (method, params = {}, sessionId) => new Promise(resolve => { pending.set(++id, resolve); socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); });
  return {
    name: 'chrome',
    async page(phone) {
      const target = (await call('Target.createTarget', { url: 'about:blank' })).result.targetId;
      const session = (await call('Target.attachToTarget', { targetId: target, flatten: true })).result.sessionId;
      sessions.add(session);
      logs = [];
      const send = (method, params = {}) => call(method, params, session);
      const evaluate = async expression => (await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })).result?.result?.value;
      await send('Runtime.enable');
      await send('Page.enable');
      await send('Page.addScriptToEvaluateOnNewDocument', { source: CAPTURE });
      if (phone) {
        await send('Emulation.setDeviceMetricsOverride', { width: 428, height: 746, deviceScaleFactor: 3, mobile: true, screenOrientation: { type: 'portraitPrimary', angle: 0 } });
        await send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
        await send('Emulation.setUserAgentOverride', { userAgent: IPHONE, platform: 'iPhone' });
      } else await send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 720, deviceScaleFactor: 1, mobile: false });
      const centre = async selector => evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e||!e.offsetParent)return null;const r=e.getBoundingClientRect();return [r.left+r.width/2,r.top+r.height/2]})()`);
      return {
        goto: url => send('Page.navigate', { url }),
        evaluate,
        async tap(selector, share) {
          let at = await centre(selector);
          if (!at) return false;
          if (share !== undefined) at = await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return [r.left+r.width*${share},r.top+r.height/2]})()`);
          await send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: at[0], y: at[1] }] });
          await sleep(60);
          await send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
          return true;
        },
        async key(code, key) {
          await evaluate(`document.querySelector('#game')?.focus?.()`);
          for (const type of ['keyDown', 'keyUp']) { await send('Input.dispatchKeyEvent', { type, code, key, windowsVirtualKeyCode: key.toUpperCase().charCodeAt(0) }); await sleep(80); }
        },
        screenshot: async () => Buffer.from((await send('Page.captureScreenshot', { format: 'png' })).result.data, 'base64'),
        logs: () => logs,
        close: () => call('Target.closeTarget', { targetId: target }),
      };
    },
    async close() { socket.close(); browser.kill('SIGTERM'); await sleep(500); await rm(profile, { recursive: true, force: true }); },
  };
}

// ----- WebKit through playwright-core (HOTDOG_PLAYWRIGHT), as an iPhone.
async function webkit(dir) {
  const pw = createRequire(join(process.cwd(), 'x.js'))(dir);
  // (No sound: WebKit is given no audio server to reach.)
  const browser = await pw.webkit.launch({ headless: true, env: { ...process.env, PULSE_SERVER: 'unix:/nonexistent-hotdog-check' } });
  return {
    name: 'webkit',
    async page() {
      const device = { ...pw.devices['iPhone 14 Plus'] };
      delete device.defaultBrowserType;
      const context = await browser.newContext(device);
      await context.addInitScript(CAPTURE);
      const page = await context.newPage(), logs = [];
      page.on('console', m => logs.push(`${m.type()}: ${m.text()}`));
      page.on('pageerror', e => logs.push(`exception: ${e.message}`));
      page.on('crash', () => logs.push('exception: the page crashed'));
      return {
        goto: url => page.goto(url, { waitUntil: 'commit' }),
        evaluate: expression => page.evaluate(expression),
        async tap(selector, share) {
          const box = await page.locator(selector).first().boundingBox().catch(() => null);
          if (!box) return false;
          await page.touchscreen.tap(box.x + box.width * (share ?? 0.5), box.y + box.height / 2);
          return true;
        },
        key: (code, key) => page.keyboard.press(key),
        screenshot: () => page.screenshot(),
        logs: () => logs,
        close: () => context.close(),
      };
    },
    close: () => browser.close(),
  };
}

/** One replay watched in one browser, with a mouse or as a phone: what went wrong (nothing: it plays). */
async function watch(browser, id, phone) {
  const name = `${id}-${browser.name}-${phone ? 'phone' : 'desktop'}`, problems = [], samples = [];
  const page = await browser.page(phone);
  const sample = async () => { const s = await page.evaluate(SAMPLE); samples.push({ t: Date.now(), ...s }); return s; };
  const slow = browser.name === 'webkit';
  try {
    await page.goto(new URL(`?replay=${encodeURIComponent(id)}&norecord&nopov`, base).href);
    let s;
    for (let i = 0; i < 600; i++) { await sleep(250); s = await page.evaluate(SAMPLE); if (/^(playing|failed)$/.test(s.st?.state ?? '') || s.note) break; }
    if (s.st?.state !== 'playing') { problems.push(s.st?.state === 'failed' || s.note ? `it cannot be played: ${s.note || s.st.message}` : `it never started (${s.st?.state ?? 'no word from the game'})`); return { name, problems, samples }; }
    await sleep(3000);
    // (The pictures are judged together at the end: a round's opening, or a flat wall up close, can be bare for a moment.)
    const spreads = [], shot = async label => { const png = await page.screenshot(); await writeFile(join(out, `${name}-${label}.png`), png); spreads.push(detail(png)); };
    // Its time goes on.
    const a = await sample();
    await sleep(2000);
    const b = await sample();
    if (!(b.st.clock > a.st.clock)) problems.push(`its time does not go on (${a.st.clock} ms, then ${b.st.clock} ms)`);
    await shot('playing');
    // The controls: on a phone the page's own, by touch; with a mouse the keys.
    const asked = async (what, test, wait = 1200) => { await sleep(wait); const now = await sample(); if (!test(now.st)) problems.push(`${what} did nothing (${JSON.stringify({ clock: now.st.clock, paused: now.st.paused, speed: now.st.speed, mode: now.st.mode })})`); return now; };
    if (phone) {
      if (!b.bar) problems.push('the replay\'s own controls are not shown');
      if (b.buttons) problems.push(`the game's own buttons show over the replay (${b.buttons})`);
      if (!await page.tap('.trp-play')) problems.push('no Pause button to tap');
      const paused = await asked('a tap on Pause', st => st.paused);
      await sleep(1000);
      const still = await sample();
      if (still.st.clock !== paused.st.clock) problems.push('paused, its time still goes on');
      await page.tap('.trp-play');
      await asked('a tap on Play', st => !st.paused);
      await page.tap('.trp-view');
      await asked('a tap on the view', st => st.mode === 'follow');
      await page.tap('.trp-track', 0.5);
      await asked('a tap half way along the timeline', st => Math.abs(st.clock - st.duration / 2) < 4000, 2500);
      await page.tap('.trp-speeds [data-ask="speed:3"]');
      await asked('a tap on 2x', st => st.speed === 3);
      await shot('controls');
    } else {
      await page.key('KeyP', 'p');
      await asked('P (pause)', st => st.paused);
      await page.key('KeyP', 'p');
      await asked('P (play)', st => !st.paused);
      await page.key('BracketRight', ']');
      await asked('] (faster)', st => st.speed === 3);
    }
    // At 2x for a while: the memory stays put.
    const start = await sample(), from = Date.now();
    let settled = null, most = start.mb ?? 0;
    while (Date.now() - from < seconds * 1000) {
      await sleep(1000);
      const now = await sample();
      if (now.mb == null) continue;
      if (Date.now() - from >= 5000 && settled === null) settled = now.mb;
      most = Math.max(most, now.mb);
      if (Date.now() - from >= seconds * 500 && spreads.length < 3) await shot('middle');
    }
    const end = samples[samples.length - 1];
    if (start.mb == null) problems.push('the game\'s memory could not be read');
    else {
      if (most > ceilingMb) problems.push(`the memory went to ${most.toFixed(0)} MB (the ceiling is ${ceilingMb})`);
      if (settled !== null && most - settled > growthMb) problems.push(`the memory grew by ${(most - settled).toFixed(0)} MB while it played (from ${settled.toFixed(0)}; at most ${growthMb})`);
    }
    const went = end.st.clock - start.st.clock;
    if (!(went > (slow ? 3000 : seconds * 1000 * 0.5)) && !end.st.ended) problems.push(`in ${seconds} s at 2x its time went on ${(went / 1000).toFixed(1)} s`);
    await shot('end');
    if (Math.max(...spreads) < BARE) problems.push(`the picture is bare (spreads ${spreads.map(x => x.toFixed(0)).join(', ')})`);
    const errors = [...end.errors, ...page.logs().filter(l => /^exception|panicked/.test(l))];
    if (errors.length) problems.push(`errors: ${[...new Set(errors)].slice(0, 3).join(' | ').slice(0, 400)}`);
    console.log(`  ${name}: memory ${start.mb?.toFixed(0)} MB, then ${settled?.toFixed(0) ?? '-'} MB, at most ${most.toFixed(0)} MB; ${(went / 1000).toFixed(1)} s played in ${seconds} s`);
  } catch (error) {
    problems.push(`could not be checked: ${error.message}`);
  } finally {
    await writeFile(join(out, `${name}.json`), JSON.stringify(samples, null, 1));
    await writeFile(join(out, `${name}.console.txt`), page.logs().join('\n'));
    await page.close().catch(() => {});
  }
  return { name, problems, samples };
}

async function main() {
  const browsers = [await chrome()];
  if (process.env.HOTDOG_PLAYWRIGHT) browsers.push(await webkit(process.env.HOTDOG_PLAYWRIGHT));
  let failed = 0, unchecked = 0, runs = 0;
  try {
    for (const id of ids) {
      for (const browser of browsers) {
        for (const phone of browser.name === 'webkit' ? [true] : [false, true]) {
          runs++;
          const { name, problems } = await watch(browser, id, phone);
          const bad = problems.filter(p => !p.startsWith('could not be checked'));
          if (bad.length) failed++;
          else if (problems.length) unchecked++;
          console.log(`${name}: ${problems.length ? 'FAIL' : 'ok'}`);
          for (const p of problems) console.log(`  - ${p}`);
        }
      }
    }
  } finally {
    for (const browser of browsers) await browser.close().catch(() => {});
  }
  console.log(`${runs - failed - unchecked} of ${runs} played${failed ? `, ${failed} did not` : ''}${unchecked ? `, ${unchecked} not checked` : ''}; pictures and samples in ${out}`);
  process.exitCode = failed ? 1 : unchecked ? 2 : 0;
}

await main().catch(error => { console.error(error); process.exitCode = 2; });
