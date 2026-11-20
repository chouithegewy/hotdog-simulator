// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Browser smoke test with real WebGL and no npm dependencies. Requires Node 22+ and Chrome.
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const output = process.env.HOTDOG_CHECK_OUTPUT || join(tmpdir(), 'hotdog-maya-check');
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
  if(process.env.HOTDOG_CHECK_SCREENSHOTS!=='0')await send('Page.startScreencast',{format:'jpeg',quality:70,maxWidth:1024,maxHeight:640,everyNthFrame:1});
  const url=process.argv[2] || 'http://127.0.0.1:8137/?intro&check=maya';
  const multiplayer=new URL(url).searchParams.has('mp');
  const combatOnly=new URL(url).searchParams.has('play')||multiplayer;
  await send('Page.navigate', { url });
  if(!combatOnly){
  await waitStage(6);
  await sleep(2400);
  await shot('00-couple-greeting');
  await waitStage(1);
  await send('Page.stopScreencast');
  assert.ok(logs.some(log => log.text.includes('Breakfast cast ready')), 'Maya and her husband must be built before the fridge opens');
  assert.ok(logs.some(log => log.text.includes('Pack faces ready: five smiling sleepers')), 'Five sleeping faces and one frightened face must be present');
  assert.equal(await evaluate('document.querySelector("#action").disabled'), false);
  await shot('01-maya-at-fridge');
  await key(' ', 'Space');
  await waitStage(2);
  // Restart before combat, then exercise pointer input at phone width.
  await key('r','KeyR');
  await waitStage(0);
  await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
  await send('Emulation.setTouchEmulationEnabled', { enabled: true });
  await waitStage(1, 2);
  await shot('04-mobile');
  assert.equal(await evaluate('document.documentElement.scrollWidth <= innerWidth'), true);
  const button = await evaluate('(()=>{const r=document.querySelector("#action").getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()');
  await send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: button.x, y: button.y }] });
  await send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await waitStage(2, 2);
  await waitStage(3);
  await waitStage(5);
  await shot('03-maya-delivered');
  }
  await send('Page.stopScreencast');
  await send('Emulation.setDeviceMetricsOverride',{width:1024,height:640,deviceScaleFactor:1,mobile:false});
  await send('Emulation.setTouchEmulationEnabled',{enabled:false});
  // Multiplayer: a client without a page joins as the native game does, and both it and the page say they are ready, for
  // rounds start from the lobby.
  let nativeId,latest;
  if(multiplayer){
    // A client without a page, as the native game is: the server's game socket (the page's own server unless HOTDOG_CHECK_WS names another).
    const page=new URL(url),address=process.env.HOTDOG_CHECK_WS||page.searchParams.get('server')||`ws://${page.host}/ws`;
    nativePeer=new WebSocket(address);nativePeer.write=text=>nativePeer.send(text.trimEnd());nativePeer.destroy=()=>nativePeer.close();
    nativePeer.onmessage=event=>{const message=JSON.parse(event.data);if(message.type==='welcome')nativeId=message.id;if(message.type==='snapshot')latest=message;};
    await new Promise((resolve,reject)=>{nativePeer.onopen=resolve;nativePeer.onerror=reject;});
    // (It asks for the debug encoding: whole frames as JSON, and it sends its commands as JSON.)
    nativePeer.write(JSON.stringify({type:'join',version:12,name:'Native',json:true})+'\n');
    // Rounds start from the lobby (protocol 7): both say they are ready, the fridge opens, and the round begins.
    await waitState(s=>s.network,'browser joins the server');
    for(let i=0;i<100&&latest?.stage!=='Warmup';i++){nativePeer.write(JSON.stringify({type:'ready',ready:true})+'\n');await evaluate('window.hotdogLobby?.setReady?.(true)');await sleep(500);}
  }
  await waitState(s=>s.phase==='Warmup'||(multiplayer&&s.network&&s.phase==='Siege'),'first-person warmup');
  assert.equal(await evaluate('document.querySelector(".mission").hidden'),true,'Cinematic card must leave the combat HUD clear');
  if(process.env.HOTDOG_CHECK_CINEMATIC_ONLY==='1'){
    assert.equal(combatOnly,false,'Cinematic-only checks must start with the breakfast scene');
    assert.ok(logs.some(log=>log.text.includes('Kitchen combat: Knife')),'Delivery must lead into the knife sequence');
    assert.ok(logs.some(log=>log.text.includes('Kitchen combat: Peeling')),'The package must open before combat');
    assert.deepEqual(errors(),[]);
    console.log('PASS: breakfast greeting, Space, mobile tap, R restart, knife, package opening, and first-person combat handoff.');
  }else if(multiplayer){
    assert.equal(latest?.stage,'Warmup','both ready starts the round');
    await waitState(s=>s.network&&latest?.players.filter(player=>player.human).length===2,'browser and native peer join the same round');
    await send('Input.dispatchMouseEvent',{type:'mousePressed',x:512,y:320,button:'left',clickCount:1});
    await waitState(s=>s.ammo[1]<3,'browser fires while mouse is held');
    await send('Input.dispatchMouseEvent',{type:'mouseReleased',x:512,y:320,button:'left',clickCount:1});
    await waitState(s=>s.ammo[1]===latest.players.find(player=>player.id!==nativeId)?.ammo[1],'browser shot acknowledged by server');
    assert.ok(latest.players.find(player=>player.id!==nativeId).ammo[1]<3,'Native peer must see browser ammunition');
    nativePeer.write(JSON.stringify({type:'input',commands:[{seq:1,weapon:1,fire:true}]})+'\n');
    await sleep(600);
    assert.equal(latest.players.find(player=>player.id===nativeId).ammo[1],2,'Native shot must be accepted');
    await shot('09-multiplayer');
    nativePeer.destroy();nativePeer=undefined;
    await waitState(s=>s.roster?.find(player=>player.id===nativeId)?.bot===true,'a bot takes over the disconnected native peer\'s hotdog');
    assert.deepEqual(errors(),[]);
    console.log('PASS: rendered browser joins a native-style peer on the game socket, authoritative firing replicates both ways, and a bot takes over when it leaves.');
  }else{
  await shot('05-combat-warmup');
  await send('Input.dispatchMouseEvent',{type:'mousePressed',x:512,y:320,button:'left',clickCount:1});
  await waitState(s=>s.ammo[1]<3,'pistol firing spends ammunition');
  await send('Input.dispatchMouseEvent',{type:'mouseReleased',x:512,y:320,button:'left',clickCount:1});
  await moveTo(1.10,2.55);
  await key('e','KeyE');
  await waitState(s=>s.weapon===2&&s.ammo[2]===4,'sniper pickup');
  await send('Input.dispatchMouseEvent',{type:'mousePressed',x:512,y:320,button:'right',clickCount:1});
  await send('Input.dispatchMouseEvent',{type:'mouseReleased',x:512,y:320,button:'right',clickCount:1});
  await waitState(s=>s.scoped,'sniper scope');
  await shot('06-sniper-scope');
  await key('2','Digit2');
  await waitState(s=>s.weapon===1&&!s.scoped,'weapon switching exits scope');
  await waitState(s=>s.phase==='Siege','microwave countdown starts the siege');
  await shot('07-zombie-siege');
  await moveTo(1.47,2.40,.025);
  await waitState(s=>s.infected,'toaster turret infects a grounded player');
  await waitState(s=>s.phase==='RoundOver','infection ends offline round');
  await key('Enter','Enter');
  await waitState(s=>s.phase==='Warmup'&&!s.infected&&s.weapon===1&&s.ammo[1]===3&&!s.scoped,'Enter resets round and loadout');
  await shot('08-new-round');
  assert.deepEqual(errors(), []);
  console.log('PASS: '+(combatOnly?'':'Breakfast, mobile tap, cinematic restart, and combat handoff; ')+'movement, firing, pickups, scope, siege, turret infection, and round restart.');
  }
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
