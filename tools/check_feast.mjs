// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Browser smoke test with real WebGL and no npm dependencies. Requires Node 22+ and Chrome.
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const output = process.env.HOTDOG_CHECK_OUTPUT || join(tmpdir(), 'hotdog-feast-check');
const tempRoot = process.env.HOTDOG_CHECK_TEMP || tmpdir();
await mkdir(tempRoot, { recursive: true });
const profile = await mkdtemp(join(tempRoot, 'hotdog-maya-browser-'));
await mkdir(output, { recursive: true });
const logs = [];
let browserLog = '';
const browser = spawn(process.env.CHROME_PATH || '/opt/google/chrome/chrome', [
  '--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-extensions',
  '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
  '--ozone-platform=headless', '--enable-unsafe-swiftshader',
  '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1',
  '--user-data-dir=' + profile, 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
browser.stderr.on('data', data => { browserLog = (browserLog + data).slice(-30000); });
let socket;
let nativePeer;
const greetingFrames=[];
let recordGreeting=false;
try {
  let port;
  for (let attempt = 0; attempt < 100; attempt++) {
    try { port = Number((await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]); break; } catch {}
    if (browser.exitCode !== null) throw new Error('Chrome exited: ' + browserLog);
    await sleep(200);
  }
  assert.ok(port, 'Chrome must start its debugging endpoint');
  const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(tabs.find(tab => tab.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0;
  const pending = new Map();
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const request = pending.get(message.id);
      if (request) {
        pending.delete(message.id);
        message.error ? request.reject(message.error) : request.resolve(message.result);
      }
      return;
    }
    if (message.method === 'Runtime.consoleAPICalled') {
      const text = message.params.args.map(arg => arg.value ?? arg.description ?? '').join(' ');
      logs.push({ type: message.params.type, text });
      if (text.includes('Breakfast cast ready')) console.log('Breakfast cast ready.');
      if (text.includes('Maya breakfast shift: Greeting')) recordGreeting=true;
      if (text.includes('Maya breakfast shift: Waiting')) recordGreeting=false;
    }
    if (message.method === 'Page.screencastFrame') {
      socket.send(JSON.stringify({id:++id,method:'Page.screencastFrameAck',params:{sessionId:message.params.sessionId}}));
      if(recordGreeting&&greetingFrames.length<64) greetingFrames.push(Buffer.from(message.params.data,'base64'));
    }
    if (message.method === 'Runtime.exceptionThrown') {
      logs.push({ type: 'exception', text: message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text });
    }
    if (message.method === 'Network.responseReceived') {
      const { status, url } = message.params.response;
      if (status >= 400 && !url.endsWith('/favicon.ico')) logs.push({ type: 'http-error', text: status + ' ' + url });
    }
  };
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const requestId = ++id;
    pending.set(requestId, { resolve, reject });
    socket.send(JSON.stringify({ id: requestId, method, params }));
  });
  const evaluate = async expression => {
    const response = await send('Runtime.evaluate', { expression, returnByValue: true });
    assert.ok(!response.exceptionDetails, 'Browser evaluation failed');
    return response.result.value;
  };
  const errors = () => logs.filter(log => ['exception', 'error', 'http-error'].includes(log.type) || /ERROR|panicked|Shader.*failed/.test(log.text));
  const stageNames = ['Opening', 'Waiting', 'Reaching', 'Carrying', 'SettingDown', 'Complete','Greeting'];
  const stageCount = stage => logs.filter(log => log.text.includes('Maya breakfast shift: ' + stageNames[stage])).length;
  const waitStage = async (stage, count = 1) => {
    for (let attempt = 0; attempt < 300; attempt++) {
      assert.deepEqual(errors(), [], 'Scene must run without runtime or asset errors');
      // Software rendering can outlast a short stage. Observe recorded engine
      // transitions so a successful delivery cannot be mistaken for a timeout.
      if (stageCount(stage) >= count) return;
      if (attempt % 20 === 19) console.log(`Waiting for stage ${stage} (${attempt + 1}s)`);
      await sleep(1000);
    }
    throw new Error('Timed out waiting for mission stage ' + stage);
  };
  const shot = async name => {
    if(process.env.HOTDOG_CHECK_SCREENSHOTS==='0')return;
    const result = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    await writeFile(join(output, name + '.png'), Buffer.from(result.data, 'base64'));
  };
  const key = async (key, code) => {
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code });
  };
  const state = () => evaluate('window.hotdogState');
  const waitState = async (predicate, description, timeout = 120000) => {
    const deadline=Date.now()+timeout;
    while(Date.now()<deadline){
      assert.deepEqual(errors(), [], 'Scene must run without runtime or asset errors');
      const current=await state();if(current&&predicate(current))return current;
      await sleep(200);
    }
    throw new Error('Timed out: '+description+'; last state '+JSON.stringify(await state()));
  };
  const hold = async (key,code,ms) => {
    await send('Input.dispatchKeyEvent',{type:'keyDown',key,code});await sleep(ms);
    await send('Input.dispatchKeyEvent',{type:'keyUp',key,code});await sleep(550);
  };
  const moveTo = async (x,z,tolerance=.10) => {
    for(let i=0;i<50;i++){
      const current=await state(),dx=x-current.position[0],dz=z-current.position[2];
      if(Math.hypot(dx,dz)<tolerance)return;
      const right=dx*Math.cos(current.yaw)-dz*Math.sin(current.yaw);
      const forward=-dx*Math.sin(current.yaw)-dz*Math.cos(current.yaw);
      const direction=Math.abs(right)>Math.abs(forward)?(right>0?['d','KeyD']:['a','KeyA']):(forward>0?['w','KeyW']:['s','KeyS']);
      const frameMs=await evaluate('Number(window.hotdogMetrics?.match(/([\\d.]+) ms/)?.[1])||16');
      const speed=.67*Math.min(1,50/frameMs);
      await hold(...direction,Math.min(Math.max(180,frameMs*2),Math.max(40,frameMs*1.1,Math.max(Math.abs(right),Math.abs(forward))/speed*1000)));
    }
    throw new Error('Could not reach '+JSON.stringify([x,z])+'; last state '+JSON.stringify(await state()));
  };
  await send('Runtime.enable');
  await send('Page.enable');
  await send('Network.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 1024, height: 640, deviceScaleFactor: 1, mobile: false });
  const url=process.argv[2]||'http://127.0.0.1:8137/?demo&check=feast';
  await send('Page.navigate',{url});
  const samples=[];
  const initial=await waitState(s=>s.demo&&s.roster?.length===5&&(!new URL(url).searchParams.has('mp')||s.network),'five-player demo startup');
  assert.ok(initial.roster.every(p=>p.bot),'All five demo hotdogs must be simulated');
  samples.push(initial);console.log('Five simulated hotdogs active.');
  const famine=await waitState(s=>s.phase==='FreeForAll'&&s.wave_spawned===6&&s.wave_kills===6,'one complete zombie wave followed by free for all');
  assert.match(famine.alert,/ONLY HOTDOGS/);samples.push(famine);
  console.log('One six-zombie wave cleared; food emergency and free for all active.');
  const betrayal=await waitState(s=>s.phase==='FreeForAll'&&s.friendly_fire>0&&s.roster.some(p=>p.hp<95),'friendly fire and dropping HP');samples.push(betrayal);
  await shot('01-hotdog-free-for-all');
  const eating=await waitState(s=>s.roster.some(p=>p.meals>0),'hotdogs eat one another to restore health');samples.push(eating);
  console.log('Friendly fire, hunger, and eating observed.');
  const winner=await waitState(s=>s.phase==='RoundOver'&&s.winner!==null,'last hotdog standing');
  assert.equal(winner.roster.filter(p=>p.alive).length,1);samples.push(winner);
  await key('Enter','Enter');
  await waitState(s=>s.phase==='Warmup'&&s.friendly_fire===0&&s.roster.every(p=>p.hp===100&&p.meals===0),'round replay restores all five players');
  assert.deepEqual(errors(),[]);
  await writeFile(join(output,'states.json'),JSON.stringify(samples,null,2));
  console.log('PASS: five HD player actors, one zombie wave, food emergency, friendly fire, starvation, eating, one winner, and replay.');
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  nativePeer?.destroy();
  socket?.close();
  browser.kill('SIGTERM');
  await sleep(500);
  await rm(profile, { recursive: true, force: true });
  for(const file of await readdir(output)) if(/^greeting-\d{3}\.jpg$/.test(file)) await rm(join(output,file));
  for(let i=0;i<greetingFrames.length;i++) await writeFile(join(output,`greeting-${String(i).padStart(3,'0')}.jpg`),greetingFrames[i]);
  await writeFile(join(output, 'console.json'), JSON.stringify({ logs, browserLog }, null, 2));
  console.log('Browser artifacts: ' + output);
}
