// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Screenshots of the running game from headless Chrome on the real GPU. No npm dependencies; Node 22+.
//
//   node tools/visual_shots.mjs OUT_DIR NAME=URL[@seconds[,seconds...]] ...
//
// Each URL is opened in turn; a PNG is saved at each listed number of seconds after the page reports its
// first frame (default 12). The adapter, the frame rate and any console errors are printed per page.
// HOTDOG_SHOT_SOFTWARE=1 uses Chrome's CPU renderer instead, HOTDOG_SHOT_SIZE=1280x720 sets the viewport,
// HOTDOG_SHOT_SAMPLE=20 also watches the frame time for that many seconds.
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const [out, ...jobs] = process.argv.slice(2);
if (!out || !jobs.length) { console.error('usage: visual_shots.mjs OUT_DIR NAME=URL[@s,s] ...'); process.exit(2); }
await mkdir(out, { recursive: true });
const [width, height] = (process.env.HOTDOG_SHOT_SIZE || '1280x720').split('x').map(Number);
const profile = await mkdtemp(join(process.env.HOTDOG_CHECK_TEMP || tmpdir(), 'hotdog-shots-'));
const gpu = process.env.HOTDOG_SHOT_SOFTWARE === '1'
  ? ['--enable-unsafe-swiftshader']
  : ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist'];
const browser = spawn(process.env.CHROME_PATH || '/opt/google/chrome/chrome', [
  '--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-extensions', '--no-first-run',
  '--no-default-browser-check', '--disable-background-networking', '--ozone-platform=headless', '--force-color-profile=srgb', '--hide-scrollbars', ...gpu,
  '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1', '--user-data-dir=' + profile, 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
let browserLog = '';
browser.stderr.on('data', data => { browserLog = (browserLog + data).slice(-20000); });
let socket;
try {
  let port;
  for (let attempt = 0; attempt < 100 && !port; attempt++) {
    try { port = Number((await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]); } catch {}
    if (browser.exitCode !== null) throw new Error('Chrome exited: ' + browserLog);
    if (!port) await sleep(200);
  }
  const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(tabs.find(tab => tab.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0, logs = [];
  const pending = new Map();
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id) { pending.get(message.id)?.(message); pending.delete(message.id); return; }
    if (message.method === 'Runtime.consoleAPICalled') logs.push({ type: message.params.type, text: message.params.args.map(a => a.value ?? a.description ?? '').join(' ') });
    if (message.method === 'Runtime.exceptionThrown') logs.push({ type: 'exception', text: message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text });
  };
  const send = (method, params = {}) => new Promise(resolve => { pending.set(++id, resolve); socket.send(JSON.stringify({ id, method, params })); });
  const evaluate = async expression => (await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })).result?.result?.value;
  await send('Runtime.enable');
  await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  for (const job of jobs) {
    const [name, rest] = [job.slice(0, job.indexOf('=')), job.slice(job.indexOf('=') + 1)];
    const at = rest.lastIndexOf('@');
    const url = at > 0 ? rest.slice(0, at) : rest;
    const times = (at > 0 ? rest.slice(at + 1) : '12').split(',').map(Number);
    logs = [];
    await send('Page.navigate', { url });
    let started = 0;
    for (let i = 0; i < 600 && !started; i++) { await sleep(200); if (await evaluate('window.hotdogMetrics')) started = Date.now(); }
    if (!started) { console.log(`${name}: the game never reported a frame`); logs.slice(-12).forEach(l => console.log('   ', l.type, l.text.slice(0, 400))); continue; }
    // HOTDOG_SHOT_KEYS=F6:8,F6:16 presses keys at those seconds, for example to step through the themes.
    const presses = (process.env.HOTDOG_SHOT_KEYS || '').split(',').filter(Boolean).map(p => p.split(':'));
    if (presses.length) {
      await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: width / 2, y: height / 2, button: 'left', clickCount: 1 });
      await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: width / 2, y: height / 2, button: 'left', clickCount: 1 });
    }
    for (const [key, at] of presses) setTimeout(async () => {
      await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code: key });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code: key });
    }, Math.max(0, started + Number(at) * 1000 - Date.now()));
    for (const [index, seconds] of times.entries()) {
      await sleep(Math.max(0, started + seconds * 1000 - Date.now()));
      // Node's WebSocket drops very large messages: above 1440p ask for JPEG with HOTDOG_SHOT_FORMAT=jpeg.
      const format = process.env.HOTDOG_SHOT_FORMAT === 'jpeg' ? 'jpeg' : 'png';
      const image = await send('Page.captureScreenshot', { format, quality: 85, captureBeyondViewport: false });
      const file = join(out, (times.length > 1 ? `${name}-${index + 1}` : name) + (format === 'jpeg' ? '.jpg' : '.png'));
      await writeFile(file, Buffer.from(image.result.data, 'base64'));
    }
    // HOTDOG_SHOT_SAMPLE=seconds: watch the frame time for that long and report its median.
    let pace = '';
    if (process.env.HOTDOG_SHOT_SAMPLE) {
      const frames = [];
      for (let i = 0; i < Number(process.env.HOTDOG_SHOT_SAMPLE) * 2; i++) {
        await sleep(500);
        const ms = Number((await evaluate('window.hotdogMetrics'))?.match(/([\d.]+) ms/)?.[1]);
        if (ms) frames.push(ms);
      }
      frames.sort((x, y) => x - y);
      pace = `; frame time median ${frames[frames.length >> 1]} ms, best ${frames[0]} ms, worst ${frames.at(-1)} ms over ${frames.length} samples`;
    }
    const renderer = await evaluate('document.documentElement.dataset.renderer');
    const adapter = logs.find(l => /AdapterInfo/.test(l.text))?.text.match(/name: "([^"]*)".*?device_type: (\w+).*?backend: (\w+)/);
    const visuals = logs.filter(l => /Visuals:/.test(l.text)).map(l => l.text.replace(/%c|color: [^;]*;?|font-weight: \w+;?/g, '').replace(/\s+/g, ' ').trim());
    const bad = logs.filter(l => ['exception', 'error'].includes(l.type) || /ERROR|panicked|Shader.*failed|Validation Error/.test(l.text));
    console.log(`${name}: ${renderer}; ${adapter ? `${adapter[1]} (${adapter[2]}, ${adapter[3]})` : 'adapter not logged'}; ${await evaluate('window.hotdogMetrics')}${pace}`);
    visuals.slice(0, 6).forEach(text => console.log('    ' + text.slice(0, 300)));
    [...new Set(bad.map(l => l.text.slice(0, 700)))].slice(0, 8).forEach(text => console.log('    PROBLEM ' + text));
  }
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  socket?.close();
  browser.kill('SIGTERM');
  await sleep(500);
  await rm(profile, { recursive: true, force: true });
}
