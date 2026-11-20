// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Leaves the game for another tab and comes back, in headless Chrome on the real GPU, and fails if the 3D view does not
// come back. No npm dependencies; Node 22+.
//
//   node tools/check_tab_switch.mjs BASE_URL [SCENE ...]
//
// BASE_URL is a server of the build to check (`hotdog-server --root ...`, which also has the online game). A SCENE is
// the page's own address words after `?`, for example (the default is the first two):
//   play&mode=ffa                        Free For All offline
//   mp&mode=ffa                          Free For All online (the lobby is readied up; bots fill the other places)
//   mp&mode=forts&map=pillow-fort        Forts online
//   play&mode=kungfu&map=kung-fu-dojo    the Kung Fu dojo
// and with `quality=high` (`medium`, `low`, `lowest`), `webgl2` or `webgpu` added to pick the level and the renderer.
// Each scene is checked with `check&norecord&nopov` added: once this player's hotdog is alive the gun is fired for a
// moment (`window.hotdogAim`), at once another tab takes over for AWAY seconds (HOTDOG_AWAY, 12), and the game's tab
// comes back. A hidden tab draws no frames, so the first frame back spans the whole time away (as it does after another
// workspace or window has covered the browser's). Afterwards the picture must still show the scene: one with next to
// no detail (black, or one flat colour: what a camera gone NaN shows, depending on the GPU) in the last four of six
// screenshots a second apart fails the check. The screenshots, the frame-rate line and the console go to OUT
// (HOTDOG_CHECK_OUT; otherwise a new directory in HOTDOG_CHECK_TEMP or the system's). Exits 1 if a scene failed, 2 if
// one could not be checked (the game never drew, or the picture had no detail before the switch either).
//
// Chrome is started muted, with its own profile, and without the flags that stop it throttling a hidden tab: the point
// is a hidden tab as a player's browser has it. CHROME_PATH picks the browser, HOTDOG_SHOT_SIZE the viewport (1280x720).
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const [base, ...asked] = process.argv.slice(2);
if (!base) { console.error('usage: check_tab_switch.mjs BASE_URL [SCENE ...]'); process.exit(2); }
const scenes = asked.length ? asked : ['play&mode=ffa', 'mp&mode=ffa'];
const away = Number(process.env.HOTDOG_AWAY || 12);
const [width, height] = (process.env.HOTDOG_SHOT_SIZE || '1280x720').split('x').map(Number);
const temp = process.env.HOTDOG_CHECK_TEMP || tmpdir();
const out = process.env.HOTDOG_CHECK_OUT || await mkdtemp(join(temp, 'hotdog-tab-switch-'));
await mkdir(out, { recursive: true });

/** A PNG's pixels (8 bits a channel, RGB or RGBA, as Chrome writes them). */
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

/** How much the middle of the picture (where the head-up display is no more than the crosshair and a line of text)
 * varies: its brightness averaged over squares of 8 by 8 pixels (which evens out the film grain), and the spread between
 * the brightest and the darkest of them, leaving out a fiftieth at each end (the crosshair, "YOU DIED"); and its mean
 * brightness. A scene drawn spreads over 100 at almost every moment, and seldom under 40 (Lowest's flat walls seen up
 * close); black, or one flat colour, 15 at the most. */
function detail(png) {
  const { w, h, bpp, px } = decode(png);
  const [left, top, size] = [Math.floor(w * 0.1), Math.floor(h * 0.15), 8];
  const squares = [];
  for (let y = top; y + size <= h * 0.85; y += size) {
    for (let x = left; x + size <= w * 0.9; x += size) {
      let sum = 0;
      for (let dy = 0; dy < size; dy++) {
        for (let dx = 0; dx < size; dx++) {
          const i = ((y + dy) * w + x + dx) * bpp;
          sum += 0.2126 * px[i] + 0.7152 * px[i + 1] + 0.0722 * px[i + 2];
        }
      }
      squares.push(sum / (size * size));
    }
  }
  squares.sort((a, b) => a - b);
  const at = share => squares[Math.floor(share * (squares.length - 1))];
  return { spread: at(0.98) - at(0.02), mean: squares.reduce((a, b) => a + b, 0) / squares.length };
}
/** Under this spread the scene is not drawn. */
const BARE = 30;

async function main() {
  const profile = await mkdtemp(join(temp, 'hotdog-tab-switch-chrome-'));
  const browser = spawn(process.env.CHROME_PATH || '/opt/google/chrome/chrome', [
    '--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-extensions', '--no-first-run', '--no-default-browser-check',
    '--disable-background-networking', '--ozone-platform=headless', '--force-color-profile=srgb', '--hide-scrollbars', '--mute-audio',
    '--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist',
    '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1', '--user-data-dir=' + profile, 'about:blank',
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  let browserLog = '';
  browser.stderr.on('data', data => { browserLog = (browserLog + data).slice(-20000); });
  let socket, failed = 0, unchecked = 0;
  try {
    let port, path;
    for (let attempt = 0; attempt < 100 && !port; attempt++) {
      try { [port, path] = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n'); } catch {}
      if (browser.exitCode !== null) throw new Error('Chrome exited: ' + browserLog);
      if (!port) await sleep(200);
    }
    // The browser's own connection (tabs are made and brought to the front there), with the game's tab attached to it.
    socket = new WebSocket(`ws://127.0.0.1:${port}${path}`);
    await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
    let id = 0, session = null, logs = [];
    const pending = new Map();
    socket.onmessage = event => {
      const message = JSON.parse(event.data);
      if (message.id) { pending.get(message.id)?.(message); pending.delete(message.id); return; }
      if (message.sessionId !== session) return;
      if (message.method === 'Runtime.consoleAPICalled') logs.push({ type: message.params.type, text: message.params.args.map(a => a.value ?? a.description ?? '').join(' ') });
      if (message.method === 'Runtime.exceptionThrown') logs.push({ type: 'exception', text: message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text });
    };
    const call = (method, params = {}, sessionId) => new Promise(resolve => { pending.set(++id, resolve); socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); });
    const send = (method, params = {}) => call(method, params, session);
    const evaluate = async expression => (await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })).result?.result?.value;
    const tab = (await call('Target.getTargets')).result.targetInfos.find(t => t.type === 'page').targetId;
    session = (await call('Target.attachToTarget', { targetId: tab, flatten: true })).result.sessionId;
    await send('Runtime.enable');
    await send('Page.enable');
    await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
    const line = () => evaluate(`[window.hotdogMetrics?.replace('HOTDOG · ',''),window.hotdogGraphics,document.documentElement.dataset.renderer].join(' · ')`);
    const alive = () => evaluate('(s=>!!s?.roster?.find(p=>p.id===s.id)?.alive)(window.hotdogState)');
    for (const [index, scene] of scenes.entries()) {
      const name = `${index + 1}-${scene.replace(/[^\w-]+/g, '_')}`;
      const url = new URL(`?${scene}&check&norecord&nopov`, base).href;
      logs = [];
      await send('Page.navigate', { url });
      let drawn = false;
      for (let i = 0; i < 600 && !drawn; i++) { await sleep(200); drawn = !!(await evaluate('window.hotdogMetrics')); }
      if (!drawn) { console.log(`${name}: the game never drew\n  ${url}`); unchecked++; continue; }
      if (/(^|&)mp(&|$)/.test(scene)) await evaluate('(async()=>{for(let i=0;i<120;i++){window.hotdogLobby?.setReady?.(true);await new Promise(r=>setTimeout(r,500))}})();1');
      // Into the round, alive, and a few seconds for the scene to settle.
      for (let i = 0; i < 300 && !(await alive()); i++) await sleep(200);
      await sleep(4000);
      for (let i = 0; i < 100 && !(await alive()); i++) await sleep(200);
      const shot = async label => {
        const png = Buffer.from((await send('Page.captureScreenshot', { format: 'png' })).result.data, 'base64');
        await writeFile(join(out, `${name}-${label}.png`), png);
        return { ...detail(png), line: await line() };
      };
      const before = await shot('before');
      if (before.spread < BARE) { console.log(`${name}: no scene to be seen before the switch (a spread of ${before.spread.toFixed(0)}): not checked\n  ${url}`); unchecked++; continue; }
      // The gun fired for a moment, and the switch at once: the kick is still settling (the gun's and the view's).
      const rounds = () => evaluate('(window.hotdogState?.ammo||[]).reduce((a,b)=>a+Math.max(b,0),0)');
      const loaded = await rounds();
      await evaluate('window.hotdogAim=[window.hotdogState.yaw,0,1];1');
      await sleep(600);
      await evaluate('window.hotdogAim=[window.hotdogState.yaw,0,0];1');
      const fired = loaded - await rounds();
      const other = (await call('Target.createTarget', { url: 'about:blank' })).result.targetId;
      await call('Target.activateTarget', { targetId: other });
      await sleep(500);
      const hidden = await evaluate('document.visibilityState');
      await sleep(away * 1000 - 500);
      await call('Target.activateTarget', { targetId: tab });
      await call('Target.closeTarget', { targetId: other });
      await sleep(2000);
      const after = [];
      for (let i = 0; i < 6; i++) { after.push(await shot(`after-${i + 1}`)); await sleep(1000); }
      // (The last four: a view can be bare for a moment, the death camera passing a wall at Lowest; the broken one stays.)
      const bare = after.slice(-4).every(a => a.spread < BARE);
      const errors = [...new Set(logs.filter(l => ['exception', 'error'].includes(l.type) || /panicked|ERROR/.test(l.text)).map(l => l.text.slice(0, 300)))];
      await writeFile(join(out, `${name}.console.txt`), logs.map(l => `${l.type}: ${l.text}`).join('\n'));
      if (bare) failed++;
      console.log(`${name}: ${bare ? 'FAIL: the scene is gone after the switch' : 'ok'} (${fired > 0 ? `${fired} rounds fired, then ` : 'no round fired (no gun here?), '}the tab ${hidden} for ${away} s)\n  ${url}`);
      console.log(`  before: spread ${before.spread.toFixed(0)}, mean ${before.mean.toFixed(0)} · ${before.line}`);
      after.forEach((a, i) => console.log(`  after ${i + 1}: spread ${a.spread.toFixed(0)}, mean ${a.mean.toFixed(0)} · ${a.line}`));
      errors.slice(0, 5).forEach(text => console.log(`  console: ${text}`));
    }
  } finally {
    socket?.close();
    browser.kill('SIGTERM');
    await sleep(500);
    await rm(profile, { recursive: true, force: true });
  }
  console.log(`${scenes.length - failed - unchecked} of ${scenes.length} scenes came back${failed ? `, ${failed} did not` : ''}${unchecked ? `, ${unchecked} not checked` : ''}; pictures in ${out}`);
  process.exitCode = failed ? 1 : unchecked ? 2 : 0;
}

await main().catch(error => { console.error(error); process.exitCode = 2; });
