// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The graphics menus on the frame-rate line (lobby.js, graphics): what each offers, what is disabled and why, the address a new
// renderer reloads to, what is kept, and when the line takes clicks. The game's side (src/quality.rs) is tested in Rust.
//   node --test tools/graphics.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {RENDERERS,QUALITIES,SCALES,STORE,INSECURE,FAILED_FOR,webgpuFailed,qualityKey,scaleKey,addressed,plan,webgpuBlocked,readLabel,menus,rendererAddress,choose,clickable} from '../lobby.js';

// A page on localhost with WebGPU, the WebGL 2 build running, nothing chosen yet and the game not yet reporting.
const base={search:'',saved:{},running:'webgl2',secure:true,gpu:true,adapter:null,quality:'',scale:'',label:''};
const at=(menu,value)=>menu.items.find(item=>item.value===value);
const checked=menu=>menu.items.filter(item=>item.checked).map(item=>item.value);

test('each menu offers its choices, Auto first, with the names the game uses',()=>{
 assert.deepEqual(RENDERERS.map(([v])=>v),['auto','webgpu','webgl2']);
 assert.deepEqual(QUALITIES.map(([v])=>v),['auto','high','medium','low','lowest'],'src/quality.rs Choice::key');
 assert.deepEqual(SCALES.map(([v,t])=>[v,t]),[['auto','Auto'],['100','100%'],['90','90%'],['80','80%'],['70','70%'],['60','60%'],['50','50%']]);
 const m=menus(base);
 assert.deepEqual(Object.keys(m),['renderer','quality','scale']);
 assert.deepEqual(m.renderer.items.map(i=>i.text),['Auto','WebGPU','WebGL 2']);
 assert.deepEqual(m.quality.items.map(i=>i.text),['Auto','High','Medium','Low','Lowest']);
 assert.deepEqual(m.scale.items.map(i=>i.text),['Auto','100%','90%','80%','70%','60%','50%']);
 // Nothing chosen: Auto in each, and nothing disabled or locked.
 for(const menu of Object.values(m)){assert.deepEqual(checked(menu),['auto'],menu.name);assert.equal(menu.locked,'');assert.ok(menu.items.every(i=>!i.disabled),menu.name)}
 // Before the game reports, the line shows the build, the quality chosen and no percentage yet.
 assert.deepEqual([m.renderer.text,m.quality.text,m.scale.text],['WebGL 2','Auto','--%']);
 assert.equal(at(m.renderer,'auto').now,'WebGL 2','Auto says which build it came to');
});

test('the line shows what the game reports, and the menus mark what was chosen',()=>{
 assert.deepEqual(readLabel('Auto · Low · 70%'),{auto:true,level:'Low',percent:70});
 assert.deepEqual(readLabel('Lowest · 50% (?quality)'),{auto:false,level:'Lowest',percent:50});
 assert.equal(readLabel(''),null);assert.equal(readLabel('Ultra · 70%'),null);
 let m=menus({...base,running:'webgpu',label:'Auto · Low · 70%',quality:'auto',scale:'auto'});
 assert.deepEqual([m.renderer.text,m.quality.text,m.scale.text],['WebGPU','Auto · Low','70%']);
 assert.equal(at(m.quality,'auto').now,'Low');assert.equal(at(m.scale,'auto').now,'70%');
 // A level and a resolution chosen: those are marked, and Auto no longer says what it would be.
 m=menus({...base,label:'Medium · 80%',quality:'medium',scale:'80'});
 assert.deepEqual([checked(m.quality),checked(m.scale)],[['medium'],['80']]);
 assert.deepEqual([m.quality.text,m.scale.text],['Medium','80%']);
 assert.equal(at(m.quality,'auto').now,'');assert.equal(at(m.scale,'auto').now,'');
 // What the game said wins over what is stored (F8 changes it); what is stored counts before it has said anything.
 m=menus({...base,saved:{quality:'low',scale:'60'},quality:'high'});
 assert.deepEqual([checked(m.quality),checked(m.scale)],[['high'],['60']]);
 assert.equal(m.scale.text,'60%','a fixed resolution is known before the game reports');
 // Rubbish stored is Auto.
 m=menus({...base,saved:{renderer:'vulkan',quality:'ultra',scale:'12'}});
 assert.deepEqual([checked(m.renderer),checked(m.quality),checked(m.scale)],[['auto'],['auto'],['auto']]);
});

test('WebGPU is disabled, with the reason, where the page cannot have it',()=>{
 // Plain http from another machine: browsers keep WebGPU from such a page.
 let m=menus({...base,secure:false,gpu:false});
 assert.equal(at(m.renderer,'webgpu').disabled,true);assert.equal(at(m.renderer,'webgpu').reason,'needs https');
 assert.ok(m.renderer.note.includes(INSECURE),'the whole reason, as index.html gives it');
 assert.match(INSECURE,/https:\/\/ or from localhost/);
 assert.equal(at(m.renderer,'auto').disabled,false);assert.equal(at(m.renderer,'webgl2').disabled,false);
 // A secure page in a browser without it, and one whose browser found no adapter.
 m=menus({...base,gpu:false});assert.equal(at(m.renderer,'webgpu').disabled,true);assert.match(m.renderer.note,/no WebGPU/);
 m=menus({...base,adapter:false});assert.equal(at(m.renderer,'webgpu').reason,'no adapter');
 // With an adapter, or not asked yet, it may be chosen.
 for(const adapter of [true,null])assert.equal(at(menus({...base,adapter}).renderer,'webgpu').disabled,false);
 assert.equal(webgpuBlocked({secure:true,gpu:true,adapter:true}),null);
 // A saved WebGPU this page cannot have: the page loaded WebGL 2, and the menu says Auto.
 m=menus({...base,secure:false,gpu:false,saved:{renderer:'webgpu'}});
 assert.deepEqual(checked(m.renderer),['auto']);
 assert.equal(choose(m,'renderer','webgpu','http://10.0.0.2:8137/?mp'),null,'and it cannot be chosen');
});

test('the address wins over the menus and locks them, saying why',()=>{
 let m=menus({...base,search:'?mp&webgl2'});
 assert.equal(m.renderer.locked,'Set by ?webgl2 in the address');assert.equal(m.renderer.note,m.renderer.locked);
 assert.deepEqual(checked(m.renderer),['webgl2']);assert.ok(m.renderer.items.every(i=>i.disabled));
 assert.equal(m.quality.locked,'');assert.equal(m.scale.locked,'');
 m=menus({...base,search:'?webgpu',running:'webgpu',saved:{renderer:'webgl2'}});
 assert.equal(m.renderer.locked,'Set by ?webgpu in the address');assert.deepEqual(checked(m.renderer),['webgpu']);
 // ?quality=, with the old names.
 m=menus({...base,search:'?quality=lite',saved:{quality:'low'}});
 assert.equal(m.quality.locked,'Set by ?quality=lite in the address');assert.deepEqual(checked(m.quality),['high']);
 assert.ok(m.quality.items.every(i=>i.disabled));
 assert.deepEqual(checked(menus({...base,search:'?quality=minimal'}).quality),['lowest']);
 // A quality the game does not know is no quality: nothing locked (the game ignores it too).
 assert.equal(menus({...base,search:'?quality=ultra'}).quality.locked,'');
 // ?scale=, as a percentage or a share.
 m=menus({...base,search:'?scale=0.8'});
 assert.equal(m.scale.locked,'Set by ?scale=0.8 in the address');assert.deepEqual(checked(m.scale),['80']);
 assert.equal(menus({...base,search:'?scale=75%25'}).scale.text,'75%','a share the menu does not list');
 assert.deepEqual(checked(menus({...base,search:'?scale=75'}).scale),[]);
 assert.equal(choose(menus({...base,search:'?scale=80'}),'scale','90','http://x/'),null,'a locked menu chooses nothing');
 assert.deepEqual(addressed('?mp&quality=full&scale=50'),{renderer:null,quality:'high',scale:'50'});
});

test('the quality and the resolution names read as the game reads them',()=>{
 for(const [text,key] of [['auto','auto'],[' Auto ','auto'],['80','80'],['80%','80'],['0.8','80'],['100','100'],['1','100'],['50','50'],['25','25'],['20',null],['1%',null],['120',null],['-50',null],['NaN',null],['high',null],['',null]])
  assert.equal(scaleKey(text),key,`scale ${JSON.stringify(text)}`);
 for(const [text,key] of [['Low','low'],['full','high'],['lite','high'],['minimal','lowest'],['AUTO','auto'],['ultra',null],['',null]])
  assert.equal(qualityKey(text),key,`quality ${JSON.stringify(text)}`);
});

test('a new renderer reloads the page with the address as it was, less ?webgl2 or ?webgpu',()=>{
 assert.equal(rendererAddress('http://h:8137/?mp'),'/?mp');
 assert.equal(rendererAddress('http://h:8137/?mp&webgl2'),'/?mp');
 assert.equal(rendererAddress('http://h:8137/?webgpu&mp&mode=forts&map=pillow-fort'),'/?mp&mode=forts&map=pillow-fort');
 assert.equal(rendererAddress('http://h:8137/?webgl2=&webgpu=1'),'/');
 assert.equal(rendererAddress('http://h:8137/index.html?play&webgl2#map=%7B%22a%22%3A1%7D'),'/index.html?play#map=%7B%22a%22%3A1%7D','the editor’s map stays');
 assert.equal(rendererAddress('http://h:8137/?server=ws%3A%2F%2Fx%3A1%2Fws&name=a+b&webgl2'),'/?server=ws%3A%2F%2Fx%3A1%2Fws&name=a+b','every other part kept as written');
 assert.equal(rendererAddress('http://h:8137/?webgl2x&%zz'),'/?webgl2x&%zz','only those two names go');
});

test('what a choice keeps, tells the game, or reloads',()=>{
 const m=menus({...base,running:'webgpu',adapter:true,label:'Auto · High · 100%'});
 const href='http://h:8137/?mp&webgpu';
 // The quality and the resolution: kept, and told to the game at once.
 assert.deepEqual(choose(m,'quality','low',href),{save:[STORE.quality,'low'],tell:['hotdogGraphicsChoice','low']});
 assert.deepEqual(choose(m,'scale','70',href),{save:[STORE.scale,'70'],tell:['hotdogScaleChoice','70']});
 assert.deepEqual(choose(m,'scale','auto',href),{save:['hotdog-scale','auto'],tell:['hotdogScaleChoice','auto']});
 assert.deepEqual([STORE.quality,STORE.scale,STORE.renderer],['hotdog-graphics','hotdog-scale','hotdog-renderer'],'the game reads the first two');
 // A renderer: kept, and the page reloads unless the build running is the one asked for or nothing changed.
 assert.deepEqual(choose(m,'renderer','webgl2','http://h:8137/?mp'),{save:[STORE.renderer,'webgl2'],reload:'/?mp'});
 assert.deepEqual(choose(m,'renderer','webgpu','http://h:8137/?mp'),{save:[STORE.renderer,'webgpu'],reload:null},'WebGPU is running already');
 assert.deepEqual(choose(m,'renderer','auto','http://h:8137/?mp'),{save:[STORE.renderer,'auto'],reload:null},'Auto is chosen already');
 const gl=menus({...base,saved:{renderer:'webgl2'}});
 assert.deepEqual(choose(gl,'renderer','auto','http://h:8137/?mp&map=x'),{save:[STORE.renderer,'auto'],reload:'/?mp&map=x'},'Auto may come to WebGPU');
 // Nothing for what is not there.
 assert.equal(choose(m,'quality','ultra',href),null);assert.equal(choose(m,'colour','red',href),null);
});

test('which build is loaded: the address, then the choice, then whether the browser has WebGPU',()=>{
 assert.deepEqual(plan('?webgpu','webgl2',true),{forced:'webgpu',choice:'webgpu',probe:false});
 assert.deepEqual(plan('?webgl2','webgpu',true),{forced:'webgl2',choice:'webgl2',probe:false});
 assert.deepEqual(plan('?mp','webgl2',true),{forced:null,choice:'webgl2',probe:false},'WebGL 2 chosen: no adapter asked for');
 assert.deepEqual(plan('?mp','webgpu',true),{forced:null,choice:'webgpu',probe:true});
 assert.deepEqual(plan('','',true),{forced:null,choice:'auto',probe:true});
 assert.deepEqual(plan('','auto',false),{forced:null,choice:'auto',probe:false},'no WebGPU in the browser: WebGL 2');
 assert.deepEqual(plan('','metal',true).choice,'auto');
});

test('WebGPU is preferred: Auto loads it where the browser offers an adapter, WebGL 2 elsewhere, and a choice or the address wins',()=>{
 // Nothing chosen, WebGPU in the browser: the page asks for an adapter and loads the WebGPU build if there is one (index.html).
 assert.equal(plan('?play','',true).probe,true);
 assert.equal(plan('?play','auto',true).probe,true);
 // No navigator.gpu (an old browser, or a page over plain http): never probed, so WebGL 2.
 assert.equal(plan('?play','',false).probe,false);assert.equal(plan('?play','webgpu',false).probe,false);
 // The player's choice in the menu wins, and is the page's to remember: WebGL 2 is not probed for; WebGPU is.
 assert.deepEqual(plan('?play','webgl2',true),{forced:null,choice:'webgl2',probe:false});
 assert.deepEqual(plan('?play','webgpu',true),{forced:null,choice:'webgpu',probe:true});
 // The address beats the choice either way.
 assert.equal(plan('?play&webgl2','webgpu',true).probe,false);assert.equal(plan('?play&webgpu','webgl2',true).forced,'webgpu');
 // The menu offers WebGPU wherever the page can have it, and says so on the line only once it has run.
 assert.equal(at(menus({...base,running:'webgpu',adapter:true}).renderer,'auto').now,'WebGPU');
});

test('a WebGPU build that failed to start is remembered, once: Auto stops trying it, a chosen WebGPU is tried again',()=>{
 const now=Date.UTC(2026,10,18);
 // What index.html writes (the time, in ms) counts for a week, and nothing else does: no key, rubbish, a stale or an impossible time.
 assert.equal(STORE.failed,'hotdog-webgpu-failed');assert.equal(FAILED_FOR,7*24*3600e3);
 assert.equal(webgpuFailed(String(now-1000),now),true);assert.equal(webgpuFailed(String(now-FAILED_FOR+1),now),true);
 for(const stored of ['',null,undefined,'yes','NaN','0',String(now-FAILED_FOR),String(now+FAILED_FOR)])assert.equal(webgpuFailed(stored,now),false,String(stored));
 // Auto, after a failure: WebGL 2 with no adapter asked for, so the next visit does not fail and reload again.
 assert.deepEqual(plan('?play','',true,true),{forced:null,choice:'auto',probe:false});
 assert.deepEqual(plan('?play','auto',true,true),{forced:null,choice:'auto',probe:false});
 assert.equal(plan('?play','',true,false).probe,true,'and with nothing remembered, it tries');
 // A WebGPU the player chose, or the address asked for, is not Auto's to give up: it is tried (and falls back with ?webgl2 once, which cannot loop).
 assert.equal(plan('?play','webgpu',true,true).probe,true);assert.equal(plan('?play&webgpu','',true,true).forced,'webgpu');
 // The menu says why Auto is on WebGL 2, and choosing WebGPU or Auto reloads to try again and forgets the failure; WebGL 2 does neither.
 const m=menus({...base,running:'webgl2',failed:true});
 assert.equal(m.renderer.failed,true);assert.match(m.renderer.note,/did not start.*Auto is using WebGL 2/);assert.deepEqual(checked(m.renderer),['auto']);
 assert.equal(at(m.renderer,'webgpu').disabled,false,'it can still be chosen');
 assert.deepEqual(choose(m,'renderer','webgpu','http://h:8137/?mp'),{save:[STORE.renderer,'webgpu'],reload:'/?mp',forget:STORE.failed});
 assert.deepEqual(choose(m,'renderer','auto','http://h:8137/?mp'),{save:[STORE.renderer,'auto'],reload:'/?mp',forget:STORE.failed},'Auto, which is chosen already, still reloads');
 assert.deepEqual(choose(m,'renderer','webgl2','http://h:8137/?mp'),{save:[STORE.renderer,'webgl2'],reload:null},'WebGL 2 is running');
 // Nothing to say where it does not apply: nothing remembered, WebGPU running, the address deciding, a page that cannot have WebGPU, or WebGPU chosen.
 for(const env of [{failed:false},{failed:true,running:'webgpu'},{failed:true,search:'?webgl2'},{failed:true,secure:false,gpu:false},{failed:true,saved:{renderer:'webgpu'}}]){
  const r=menus({...base,...env}).renderer;assert.equal(r.failed,false,JSON.stringify(env));assert.ok(!/did not start/.test(r.note),JSON.stringify(env))}
});

test('index.html keeps the pieces together: the remembered failure, the fallback to WebGL 2 and the two builds it loads',()=>{
 const html=readFileSync(new URL('../index.html',import.meta.url),'utf8');
 assert.ok(html.includes(`'${STORE.failed}'`),'index.html reads and writes the key the menu clears');
 assert.ok(html.includes('page.webgpuFailed(failedAt,Date.now())'),'what it remembered reaches plan()');
 assert.ok(html.includes("load(webgpu?'pkg-webgpu':'pkg')"),'the two builds are pkg/ and pkg-webgpu/ (tools/build-web.sh)');
 assert.ok(html.includes("return load('pkg')"),'a WebGPU build that cannot be had falls back to WebGL 2');
 assert.ok(html.includes("query.set('webgl2','')")&&html.includes('location.hash'),'a build that dies while starting reloads on WebGL 2 with the rest of the address, the map in the hash too');
 assert.ok(html.includes('/DeviceLost/'),'a lost GPU device is only logged, so the page watches the console for it while the build starts');
});

test('the line takes clicks in the lobby, the fridge and before a round, not during one online',()=>{
 assert.equal(clickable({online:true,view:'lobby'}),true);
 assert.equal(clickable({online:true,view:'opening'}),true,'the fridge');
 assert.equal(clickable({online:true,view:undefined}),true,'before the lobby has said anything');
 assert.equal(clickable({online:true,view:'round'}),false);
 assert.equal(clickable({online:true,view:'round',touch:'play',menu:true}),true,'the touch controls’ menu puts them away');
 // Offline there is no lobby: the corner is the page's, as the links to the other modes are, unless the touch controls are out.
 assert.equal(clickable({online:false}),true);
 for(const touch of ['play','dead','table'])assert.equal(clickable({online:false,touch}),false,touch);
 for(const touch of ['lobby','over','off'])assert.equal(clickable({online:false,touch}),true,touch);
});
