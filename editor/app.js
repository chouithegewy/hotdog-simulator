// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Map editor: wires the document model to the plan canvas, the tools, the properties panel and files.
import {KINDS,STYLES,KITS,LOOSE,RISES,THEMES,WEAPONS,AMMO_BOX,RESPAWN,LIMITS,MODES,TEAMS,TEAM_NAMES,SINK_DEPTH,SINK_MAX,MapError,blank} from '../tools/network/map.mjs';
import {Doc,LABELS,KIT_NAME,holding,rectFromDrag,resizeRect,riseFromDrag,snapPoint,edgeSnap,boxOf,alignItems,sinkAt} from './model.js';
import {View,yawTowards,zoneName} from './view.js';
import {Solid} from './solid.js';

const $=id=>document.getElementById(id);
const DRAFT='hotdog-map-editor-draft',SOLID='hotdog-map-editor-3d',GAME='hotdog-map-editor-game',MAPS='../assets/maps/',BUNDLED=MAPS+'counter-siege.json';
const LIVE_CELLS=60000;
const TOOLS=[
  {id:'select',label:'Select',key:'V',hint:'Click an object to select it, drag to move it, drag empty counter to select several.',
    icon:'<path d="M6 3l14 9-6.4 1.3L17 20l-2.8 1.4-3.3-6.6L6 19z" fill="currentColor"/>'},
  {id:'block',label:'Block',key:'B',kind:'blocks',size:[.2,.2],hint:'Drag out a block, or click to drop one. Blocks stop hotdogs, zombies and shots.',
    icon:'<rect x="4" y="6" width="16" height="13" fill="#aab6ba" stroke="currentColor" stroke-width="1.6"/><path d="M4 6l3-3h16l-3 3M20 19l3-3V3" fill="none" stroke="currentColor" stroke-width="1.4"/>'},
  {id:'deck',label:'Deck',key:'D',kind:'decks',size:[.5,.4],hint:'Drag out a raised floor. Hotdogs need a ramp to get onto it.',
    icon:'<rect x="3" y="6" width="19" height="13" fill="#8ea0a8" stroke="currentColor" stroke-width="1.6"/><path d="M3 14l8-8M8 19L21 6M15 19l7-7" stroke="#fff" stroke-width="1.6" opacity=".7"/>'},
  {id:'ramp',label:'Ramp',key:'R',kind:'ramps',size:[.6,.16],hint:'Drag from the foot of the ramp towards its top.',
    icon:'<path d="M3 19h19V6z" fill="#dfe5e5" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M10 16.5l3-2-1-3" fill="none" stroke="#c98f0a" stroke-width="1.8"/>'},
  {id:'sink',label:'Sink',key:'K',kind:'sinks',size:[.5,.4],hint:'Drag out a sink, a basin sunk into the counter. A hotdog that drops in needs a ramp out: draw one starting inside the sink.',
    icon:'<path d="M12 6.500V3.200h4.300" fill="none" stroke="currentColor" stroke-width="1.700" stroke-linecap="round" stroke-linejoin="round"/><rect x="2.500" y="6.500" width="19" height="14" rx="2" fill="#b9c2c6" stroke="currentColor" stroke-width="1.500"/><rect x="5.300" y="9.200" width="13.400" height="8.600" rx="1.200" fill="#3a454a"/><circle cx="12" cy="13.500" r="1.700" fill="#a3adb2"/>'},
  {id:'box',label:'Box prop',key:'X',kind:'props',size:[.1,.1],extra:{kind:'box'},hint:'Drag out a decorative box. Props do not block anything.',
    icon:'<rect x="5" y="6" width="14" height="13" fill="#c2935a" stroke="currentColor" stroke-width="1.6" stroke-dasharray="3 1.6"/><path d="M5 6l14 13M19 6L5 19" stroke="currentColor" stroke-width="1.1"/>'},
  {id:'turret',label:'Turret',key:'T',kind:'props',point:true,aim:true,extra:{kind:'turret'},hint:'Click to place a turret; drag to aim it.',
    icon:'<path d="M12 12L21 5" stroke="currentColor" stroke-width="3.4" stroke-linecap="round"/><circle cx="12" cy="13" r="6.5" fill="#55626a" stroke="currentColor" stroke-width="1.6"/><circle cx="12" cy="13" r="2.4" fill="#e9b824"/>'},
  {id:'loose',label:'Bowl or other loose object',key:'O',kind:'props',point:true,aim:true,hint:'Click to place a loose object hotdogs can knock about and build with (a cereal box, mug, plate, tin, cutting board, cereal bowl, roll of paper towels, salt or pepper shaker, bread bin or napkin, at its true size; or a bowl, crate, pillow, chair or tray); drag to turn it. Choose which in the panel on the right.',
    icon:'<circle cx="12" cy="12" r="9" fill="#c9d0d2" stroke="currentColor" stroke-width="1.6"/><circle cx="12" cy="12" r="4" fill="#aeb7ba" stroke="currentColor" stroke-width="1"/><path d="M5.500 9.500a7 7 0 0 1 5-4.200" fill="none" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>'},
  {id:'bin',label:'Kitchen bin',key:'I',kind:'props',point:true,aim:true,extra:{kind:'bin'},hint:'Click to stand a pedal bin on the kitchen floor, usually just off the counter; drag to turn its pedal. It is scenery: it stops nothing.',
    icon:'<rect x="9" y="2" width="6" height="4" rx="1" fill="currentColor"/><ellipse cx="12" cy="13.500" rx="8.500" ry="7.500" fill="#cfd5d8" stroke="currentColor" stroke-width="1.6"/><ellipse cx="12" cy="13.500" rx="5.800" ry="4.900" fill="none" stroke="#5d686d" stroke-width="1.100"/>'},
  {id:'start',label:'Hotdog start',key:'S',kind:'starts',point:true,aim:true,hint:'Click to place a hotdog start; drag to choose which way it faces.',
    icon:'<path d="M5 17L19 7" stroke="#e0a95a" stroke-width="9" stroke-linecap="round"/><path d="M3.5 18L20.500 6" stroke="#b4472e" stroke-width="4" stroke-linecap="round"/>'},
  {id:'spawn',label:'Zombie spawn',key:'Z',kind:'spawns',point:true,hint:'Click where zombies should appear.',
    icon:'<circle cx="12" cy="12" r="8.5" fill="#5d9447" stroke="currentColor" stroke-width="1.6"/><path d="M8.500 8h7l-7 8h7" fill="none" stroke="#fff" stroke-width="2" stroke-linejoin="round"/>'},
  {id:'pickup',label:'Weapon or ammo pickup',key:'P',kind:'pickups',point:true,hint:'Click to drop a gun or a box of ammunition; it comes back a while after it is taken. Choose which in the panel on the right.',
    icon:'<path d="M10 2l8 8-8 8-8-8z" fill="#f0b323" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><rect x="12.500" y="13" width="9.500" height="8.500" rx="1.200" fill="#78803a" stroke="currentColor" stroke-width="1.4"/><path d="M15 16.200v3.300M17.250 16.200v3.300M19.500 16.200v3.300" stroke="#e2b04a" stroke-width="1.500"/>'},
  // The Forts layout: layout any map may carry (a map with all three, the bases, the field and the bacon, can be played as Forts, whatever it was made for).
  {id:'base',label:'Team base',key:'H',kind:'zones',forts:true,size:[.5,.8],extra:{kind:'base'},hint:'Drag out a team\'s base: the bacon is carried home to it. Choose the team in the panel on the right; a team has one base, so drawing another moves it.',
    icon:'<rect x="3.500" y="5" width="17" height="15" fill="#e0b02055" stroke="#c8281e" stroke-width="1.8" stroke-dasharray="4 2.2"/><path d="M8 17V8l8 2.500-8 2.500" fill="#e0b020" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/>'},
  {id:'field',label:'Forcefield',key:'W',kind:'zones',forts:true,size:[.08,1],extra:{kind:'field'},hint:'Drag out the forcefield: a thin strip right across the counter between the bases. Nothing passes it until the build phase ends.',
    icon:'<rect x="9" y="2.500" width="6" height="19" fill="#2f8fe688" stroke="#1f5fbf" stroke-width="1.6"/><path d="M9 8l6-3M9 13l6-3M9 18l6-3" stroke="#fff" stroke-width="1.3"/>'},
  {id:'bacon',label:'Bacon',key:'N',kind:'zones',forts:true,point:true,hint:'Click where the bacon lies when the fight begins. There is one: clicking again moves it.',
    icon:'<path d="M3 9c3-3 6 3 9 0s6 3 9 0M3 15c3-3 6 3 9 0s6 3 9 0" fill="none" stroke="#b23a2c" stroke-width="4.200" stroke-linecap="round"/><path d="M3 9c3-3 6 3 9 0s6 3 9 0M3 15c3-3 6 3 9 0s6 3 9 0" fill="none" stroke="#f1d9b5" stroke-width="1.200" stroke-linecap="round"/>'},
];
const toolById=id=>TOOLS.find(tool=>tool.id===id);
const THEME_NAME={kitchen:'Kitchen by day',night:'Kitchen at night',jungle:'Jungle'};
const MODE_NAME={siege:'Siege: zombie wave, then free for all',forts:'Forts: two teams and the bacon',kungfu:'Kung Fu: hands and feet, in a ring',titan:'Titan: one giant against all the others'};
// What a gun's pickup gives, by weapon, in every gun mode whatever the map says (kitchen-sim magazine.rs PICKUP): a magazine, the power weapons fewer.
const PICKUP=[0,12,4,90,2,40,60,150],POWER=[2,4],POWER_BACK=[30,60];
// Which tool supplies what a Forts map is reported to lack.
const MISSING={'field-missing':'field','bacon-missing':'bacon','base-missing':'base'};
const RISE_NAME={'+x':'Right (+x)','-x':'Left (−x)','+z':'Down the plan (+z)','-z':'Up the plan (−z)'};
const OVERLAY_LEGEND={
  reach:[['#3f7d34','A hotdog can walk here from a start'],['#c4321c','Room to stand, but no way to get there']],
  zombie:[['#467e28','Close to a zombie spawn'],['#dc3e28','Far from every spawn'],['#77858a','Zombies cannot arrive']],
  height:[['#1f5fbf','Darker blue is higher floor'],['#b0541e','Below the counter: a sink\'s floor']],
  nav:[['#77858a','Cells zombies treat as blocked']],
};

const canvas=$('plan'),view=new View(canvas),doc=new Doc();
let solid=null;
try{solid=Solid.create($('solid'))}catch(error){console.warn('3D view unavailable:',error.message)}
const ui={tool:'select',snap:true,step:.01,overlay:'none',hover:null,guides:{x:null,z:null},marquee:null,ghost:null,readout:null,
  pointer:null,inside:false,space:false,weapon:1,loose:'bowl',team:TEAMS[0],badges:new Map,liveOverlay:true,solid:false,solidStale:true};
let drag=null;

// ---- small helpers ----------------------------------------------------------------------------------------------
const fmt=v=>String(+(+v).toFixed(4));
const metres=v=>(+v).toFixed(v<1?3:2).replace(/0$/,'');
const el=(tag,props={},...children)=>{
  const node=document.createElement(tag);
  for(const [key,value] of Object.entries(props)){
    if(key==='class')node.className=value;
    else if(key==='data')Object.assign(node.dataset,value);
    else if(key.startsWith('on'))node.addEventListener(key.slice(2),value);
    else if(value!==false&&value!==null&&value!==undefined)node[key]=value;
  }
  node.append(...children.filter(child=>child!==null&&child!==false));
  return node;
};
let toastTimer=0;
function toast(message,kind='info'){
  const box=$('toast');
  box.textContent=message;box.className=`toast ${kind}`;box.hidden=false;
  clearTimeout(toastTimer);toastTimer=setTimeout(()=>{box.hidden=true},kind==='error'?6500:2600);
}
/** Reports a refused edit; returns whether it went through. */
const settle=result=>{if(!result.ok)toast(result.error,'error');return result.ok};
let queued=false;
function redraw(){
  if(queued)return;
  queued=true;
  requestAnimationFrame(()=>{
    queued=false;
    view.draw({doc,overlay:ui.overlay,step:ui.snap?ui.step:0,hover:ui.hover,selection:doc.selection,guides:ui.guides,marquee:ui.marquee,ghost:ui.ghost,readout:ui.readout,badges:ui.badges,liveOverlay:ui.liveOverlay});
    if(ui.solid&&solid){
      if(ui.solidStale){solid.setScene(doc.map,doc.selection);ui.solidStale=false}
      solid.draw();
    }
  });
}
/** Shows or hides the 3D view beside the plan. */
function setSolid(on){
  ui.solid=!!on&&!!solid;
  $('view3d').setAttribute('aria-pressed',String(ui.solid));
  $('solidbox').hidden=!ui.solid;$('stage').classList.toggle('split',ui.solid);
  try{localStorage.setItem(SOLID,ui.solid?'1':'0')}catch{}
  view.resize();
  if(ui.solid){solid.resize();solid.frame(frameBox());ui.solidStale=true}
  redraw();
}
const local=event=>{const box=canvas.getBoundingClientRect();return [event.clientX-box.left,event.clientY-box.top]};
/** What Fit frames, in the plan and in 3D: the counter, and the bins standing on the floor beside it. */
function frameBox(){
  const b=doc.map.bounds,bins=boxOf(doc.map.props.filter(prop=>prop.kind==='bin'));
  return bins?[Math.min(b[0],bins[0]-.07),Math.min(b[1],bins[1]-.07),Math.max(b[2],bins[2]+.07),Math.max(b[3],bins[3]+.07)]:b;
}
const gridCells=()=>{const {bounds:b,nav}=doc.map;return Math.ceil((b[2]-b[0])/nav.cell)*Math.ceil((b[3]-b[1])/nav.cell)};
/** Height of whatever a point object would sit on: the floor there, or the top of a block under it (never below the counter top: in a sink, it falls in). */
function surfaceAt(x,z){
  let top=Math.max(0,doc.level.height(x,z));
  for(const block of doc.map.blocks)if(x>=block.rect[0]&&x<=block.rect[2]&&z>=block.rect[1]&&z<=block.rect[3]&&block.height>top)top=block.height;
  return top;
}
const typing=event=>event.target instanceof HTMLElement&&event.target.closest('input,select,textarea');

// ---- problems and overlays: recomputed at once on small maps, after a pause on big ones --------------------------
let analysisTimer=0;
function refreshProblems(){
  const list=$('issues'),issues=doc.analysis.issues,over=doc.overLimit,badges=new Map;
  list.replaceChildren();
  for(const message of over)list.append(el('li',{class:'error'},el('div',{class:'plain'},el('span',{class:'mark'}),el('span',{},`${message}. The game will refuse this map.`))));
  for(const issue of issues){
    const item=doc.map[issue.kind]?.[issue.index];
    if(item&&issue.code!=='few-starts'&&badges.get(item.id)!=='error')badges.set(item.id,issue.severity);
    // Something the map lacks has nothing to show: its button takes up the tool that adds it.
    const lacking=MISSING[issue.code];
    list.append(el('li',{class:issue.severity},el('button',{type:'button',title:lacking?`Take up the ${toolById(lacking).label} tool`:'Show this on the plan',onclick:()=>{
      if(lacking){const team=TEAMS[TEAM_NAMES.findIndex(name=>issue.message.includes(name))];if(team)ui.team=team;doc.select([]);setTool(lacking);return}
      if(!item||!doc.find(item.id))return;
      doc.select([item.id]);reveal(item);
    }},el('span',{class:'mark'}),el('span',{},issue.message))));
  }
  const errors=over.length+issues.filter(issue=>issue.severity==='error').length,total=over.length+issues.length;
  if(!total)list.append(el('li',{class:'fine'},'None. Every start, spawn, pickup and deck can be reached.'));
  const count=$('count');
  count.textContent=String(total);count.className=`count ${errors?'bad':total?'warn':''}`;
  ui.badges=badges;ui.liveOverlay=true;
  const stats=doc.analysis.stats;
  $('stats').textContent=`Navigation grid ${stats.cols} × ${stats.rows}`;
  redraw();
}
function scheduleProblems(){
  clearTimeout(analysisTimer);
  if(gridCells()<=LIVE_CELLS&&!(doc.dragging&&doc.map.blocks.length>400)){refreshProblems();return}
  ui.liveOverlay=false;
  analysisTimer=setTimeout(refreshProblems,180);
}
function reveal(item){
  const box=item.rect??[item.p[0],item.p[1],item.p[0],item.p[1]],x=view.sx((box[0]+box[2])/2),y=view.sy((box[1]+box[3])/2);
  if(x<40||x>view.w-20||y<40||y>view.h-20){view.cx=(box[0]+box[2])/2;view.cz=(box[1]+box[3])/2}
  redraw();
}

// ---- top bar ----------------------------------------------------------------------------------------------------
function syncBar(){
  if(document.activeElement!==$('name'))$('name').value=doc.map.name;
  $('undo').disabled=!doc.canUndo;$('redo').disabled=!doc.canRedo;
  $('dirty').hidden=!doc.dirty;
  document.title=`${doc.dirty?'• ':''}${doc.map.name} · Hotdog Simulator map editor`;
}
function setTool(id){
  if(drag)endDrag(true);
  ui.tool=id;ui.ghost=null;ui.hover=null;
  for(const button of $('tools').querySelectorAll('button'))button.setAttribute('aria-pressed',String(button.dataset.tool===id));
  $('hint').textContent=toolById(id).hint;
  // The base tool's team is chosen in the panel, which a selection would cover; it starts on the one team that has no base yet.
  if(id==='base'){
    const lacking=TEAMS.filter(team=>!doc.map.zones.some(each=>each.kind==='base'&&each.team===team));
    if(lacking.length===1)ui.team=lacking[0];
    if(doc.selection.length)doc.select([]);
  }
  canvas.style.cursor=id==='select'?'default':'crosshair';
  renderInspector();redraw();
}
function setOverlay(mode){
  ui.overlay=mode;$('overlay').value=mode;
  const legend=$('legend'),rows=OVERLAY_LEGEND[mode];
  legend.hidden=!rows;
  if(rows)legend.replaceChildren(...rows.map(([colour,text])=>el('span',{},el('i',{style:`background:${colour}`}),text)));
  redraw();
}
/** The Forts layout's tools show on every map: its zones are layout any map may carry (a map with them all can be played as Forts). */
function syncTools(){
  for(const node of $('tools').querySelectorAll('.forts'))node.hidden=false;
}
function fit(){const box=frameBox();view.fit(box);if(ui.solid)solid.frame(box);redraw()}

// ---- files ------------------------------------------------------------------------------------------------------
const fileName=()=>(doc.map.name.toLowerCase().replace(/[^a-z0-9]+/g,'-').replace(/^-+|-+$/g,'')||'map')+'.json';
const discardOk=()=>!doc.dirty||window.confirm('This map has changes you have not downloaded. Discard them?');
function openText(text,name){
  try{doc.load(text)}catch(error){
    if(!(error instanceof MapError))throw error;
    toast(`Could not open ${name}: ${error.message}`,'error');return false;
  }
  fit();toast(`Opened ${name}`);return true;
}
async function openFile(file){
  if(!file||!discardOk())return;
  openText(await file.text(),file.name);
}
function download(){
  const url=URL.createObjectURL(new Blob([doc.toJSON()],{type:'application/json'})),link=el('a',{href:url,download:fileName()});
  document.body.append(link);link.click();link.remove();
  setTimeout(()=>URL.revokeObjectURL(url),2000);
  doc.markSaved();syncBar();
  const over=doc.overLimit,errors=doc.analysis.issues.filter(issue=>issue.severity==='error').length;
  if(over.length)toast(`Downloaded ${fileName()}, but the game will refuse it: ${over[0]}`,'error');
  else if(errors)toast(`Downloaded ${fileName()} with ${errors} unsolved ${errors===1?'problem':'problems'}`,'error');
  else toast(`Downloaded ${fileName()}`);
}
// ---- playing: the game opens in its own tab with the map in its address, then receives every later edit ----------
let gameWindow=null,gameTimer=0;
/** Where the game is served. By default the same server as the editor; the Counter panel can point it elsewhere. */
function gameAddress(){
  let stored=null;
  try{stored=localStorage.getItem(GAME)}catch{}
  return stored||`${location.origin}/index.html`;
}
function play(){
  const over=doc.overLimit;
  if(over.length){toast(`The game cannot load this map: ${over[0]}`,'error');return}
  let url;
  try{url=new URL(gameAddress(),location.href)}catch{toast('The game address in the Counter panel is not a web address','error');return}
  // A changed address makes the tab reload even when it is already showing the game.
  url.searchParams.set('play','');url.searchParams.set('edit',String(Date.now()));
  url.hash='map='+encodeURIComponent(doc.toJSON());
  gameWindow=window.open(url.href.replace('play=&','play&'),'hotdog-game');
  if(!gameWindow){toast('The browser blocked the new tab. Allow pop-ups for this page, then press Play again.','error');return}
  const errors=doc.analysis.issues.filter(issue=>issue.severity==='error').length;
  if(errors)toast(`Playing with ${errors} unsolved ${errors===1?'problem':'problems'}`,'error');
  else toast('Opened the game in another tab');
}
function sendToGame(){
  if(!gameWindow||gameWindow.closed){gameWindow=null;return}
  clearTimeout(gameTimer);
  gameTimer=setTimeout(()=>{
    if(!gameWindow||gameWindow.closed||doc.overLimit.length)return;
    try{gameWindow.postMessage({type:'hotdog-map',text:doc.toJSON()},new URL(gameAddress(),location.href).origin)}catch{}
  },250);
}
let draftTimer=0;
function writeDraft(){
  clearTimeout(draftTimer);draftTimer=0;
  try{localStorage.setItem(DRAFT,JSON.stringify({text:doc.toJSON(),dirty:doc.dirty}))}catch{}
}
function saveDraft(){
  clearTimeout(draftTimer);
  draftTimer=setTimeout(writeDraft,400);
}
/** Opens one of the maps stored with the game, by file name. */
async function openStored(file){
  let response;
  try{response=await fetch(MAPS+file)}catch{}
  if(!response?.ok){toast(`Could not fetch ${file} from the server`,'error');return false}
  return openText(await response.text(),file);
}
/** The Open dialog: maps stored with the game when the server can list them, otherwise straight to a file. */
async function showOpener(){
  let maps=null;
  try{const response=await fetch(MAPS);if(response.ok&&response.headers.get('content-type')?.includes('json'))maps=await response.json()}catch{}
  if(!Array.isArray(maps)||!maps.length){$('file').click();return}
  $('maplist').replaceChildren(...maps.map(map=>el('li',{},el('button',{type:'button',onclick:async ()=>{
    if(!discardOk())return;
    $('opener').close();openStored(map.file);
  }},el('span',{},map.name),el('small',{},map.file)))));
  $('opener').showModal();
}
async function start(){
  let draft=null;
  try{draft=JSON.parse(localStorage.getItem(DRAFT)??'null')}catch{}
  // A link such as ?map=counter-siege-v2 opens that stored map.
  const wanted=new URLSearchParams(location.search).get('map');
  if(wanted&&/^[\w-]+$/.test(wanted)&&(!draft?.dirty||window.confirm(`Open ${wanted} and discard the changes you had not downloaded?`))){
    try{const response=await fetch(`${MAPS}${wanted}.json`);if(response.ok){doc.load(await response.text());return null}}catch{}
    return `Could not open the map called ${wanted}`;
  }
  if(draft?.text)try{doc.load(draft.text,{saved:!draft.dirty});return draft.dirty?'Picked up where you left off':null}catch{}
  try{
    const response=await fetch(BUNDLED);
    if(response.ok){doc.load(await response.text());return null}
  }catch{}
  return 'Could not load the bundled map, so this is a blank one';
}

// ---- properties panel -------------------------------------------------------------------------------------------
function numberField(label,key,value,{step=.01,min,max,unit='m',wide=false,mixed=false}={},commit){
  const input=el('input',{type:'number',step,min,max,value:mixed?'':fmt(value),placeholder:mixed?'Mixed':'',data:{key}});
  input.addEventListener('change',()=>{
    const v=input.valueAsNumber;
    if(Number.isFinite(v))commit(v);
    renderInspector();
  });
  return el('label',{class:`field${wide?' wide':''}`},label,unit?el('span',{class:'unit'},input,unit):input);
}
function selectField(label,key,value,options,commit,{wide=true}={}){
  const select=el('select',{data:{key}},...options.map(option=>Array.isArray(option[1])
    ?el('optgroup',{label:option[0]},...option[1].map(([v,text])=>el('option',{value:v,selected:String(v)===String(value)},text)))
    :el('option',{value:option[0],selected:String(option[0])===String(value)},option[1])));
  if(value===null)select.prepend(el('option',{value:'',selected:true,disabled:true},'Mixed'));
  select.addEventListener('change',()=>{commit(select.value);renderInspector()});
  return el('label',{class:`field${wide?' wide':''}`},label,select);
}
const title=text=>text[0].toUpperCase()+text.slice(1);
// What the game calls each loose object (crates/kitchen-sim/src/map.rs LOOSE_NAMES), where its name in the file is short.
const LOOSE_TITLE={cereal:'Cereal box',cup:'Mug',tin:'Tin can',board:'Cutting board',dish:'Cereal bowl',roll:'Paper towel roll',salt:'Salt shaker',pepper:'Pepper shaker',breadbox:'Bread bin'};
const looseTitle=kind=>LOOSE_TITLE[kind]??title(kind);
const styleOptions=kits=>{
  const plain=STYLES.map(style=>[style,title(style)]);
  return kits?[['Materials',plain],['Kitchen things',KITS.map(kit=>[kit,KIT_NAME[kit]??title(kit)])]]:plain;
};
// A ramp is the metal one with yellow edges, or a plank of a material.
const rampOptions=()=>[['','Metal ramp'],['Plank of',STYLES.map(style=>[style,title(style)])]];
const degrees=yaw=>Math.round(yaw*1800/Math.PI)/10,radians=deg=>deg*Math.PI/180;
// What a pickup can hold: one of the guns, or (weapon 0) a box of ammunition that fills every gun its taker holds.
const PICKUP_NAME=weapon=>weapon===AMMO_BOX?'Ammo box':WEAPONS[weapon]??`Weapon ${weapon}`;
const weaponOptions=()=>[['Guns',WEAPONS.flatMap((name,index)=>index===AMMO_BOX?[]:[[index,name]])],['Ammunition',[[AMMO_BOX,PICKUP_NAME(AMMO_BOX)]]]];
/** Seconds a taken pickup holding `weapon` takes to come back: the game's rules', the same on every map (a map's own `respawn` is passed over), and
 * for a power weapon a time between POWER_BACK[0] and POWER_BACK[1] drawn each time it is taken. */
const comeback=weapon=>POWER.includes(weapon)?null:RESPAWN[weapon===AMMO_BOX?1:0];
const seconds=v=>`${fmt(v)} s`;
const teamOptions=()=>TEAMS.map((team,t)=>[team,TEAM_NAMES[t]]);
/** The team whose side of the forcefield a point is on, on a Forts map. */
const teamAt=p=>{const level=doc.level;return level.fort?TEAMS[level.side(p[0],p[1])===level.fort.sides[0]?0:1]:undefined};
/** What an object is called in the panel's heading. */
const nameOf=found=>found.kind==='zones'?(found.item.kind==='base'?zoneName(found.item):`${zoneName(found.item)}${doc.map.zones.filter(zone=>zone.kind===found.item.kind).length>1?` ${found.index+1}`:''}`)
  :found.kind==='starts'?`Hotdog start ${found.index+1}`:found.kind==='spawns'?`Zombie spawn ${found.index+1}`
  :found.kind==='blocks'&&KIT_NAME[found.item.style]?`${KIT_NAME[found.item.style]} (block ${found.index+1})`
  :found.kind==='ramps'&&found.item.style?`Plank ramp ${found.index+1}`
  :`${found.kind==='props'?(found.item.rect?'Box prop':title(found.item.kind)):LABELS[found.kind]} ${found.index+1}`;

function itemFields(found){
  const {kind,item}=found,id=item.id,set=patch=>settle(doc.patch(id,patch)),fields=[];
  if(item.rect){
    const [x0,z0,x1,z1]=item.rect,w=x1-x0,d=z1-z0;
    fields.push(
      numberField('Left edge (x)','x',x0,{},v=>set({rect:[v,z0,v+w,z1]})),
      numberField('Top edge (z)','z',z0,{},v=>set({rect:[x0,v,x1,v+d]})),
      numberField('Width','w',w,{min:.001},v=>set({rect:[x0,z0,x0+v,z1]})),
      numberField('Depth','d',d,{min:.001},v=>set({rect:[x0,z0,x1,z0+v]})));
  }else fields.push(
    numberField('Position x','x',item.p[0],{},v=>set({p:[v,item.p[1]]})),
    numberField('Position z','z',item.p[1],{},v=>set({p:[item.p[0],v]})));
  // A ramp's foot may be below the counter, down a sink (a board propped in it); one that names a style is a plank of it, and choosing the metal ramp takes the style out.
  if(kind==='ramps')fields.push(
    numberField('Foot height','base',item.base,{min:-SINK_MAX},v=>set({base:v})),
    numberField('Top height','height',item.height,{min:.01},v=>set({height:v})),
    selectField('Climbs towards','rise',item.rise,RISES.map(rise=>[rise,RISE_NAME[rise]]),v=>set({rise:v})),
    selectField('Looks like','style',item.style??'',rampOptions(),v=>set({style:v||null})));
  else if(kind==='sinks')fields.push(numberField('Floor below the counter','depth',item.depth,{min:.05,max:SINK_MAX,wide:true},v=>set({depth:v})));
  else{
    if('base' in item)fields.push(numberField('Sits at height','base',item.base,{min:0},v=>set({base:v})));
    if('height' in item)fields.push(numberField(kind==='decks'?'Floor height':'Height','height',item.height,{min:.005},v=>set({height:v})));
  }
  if(kind==='zones'&&item.kind==='base')fields.push(selectField('Team','team',item.team,teamOptions(),v=>{ui.team=v;set({team:v})}));
  if(kind==='starts'&&(doc.map.mode==='forts'||item.team||doc.map.zones.length))fields.push(selectField('Team','team',item.team??'',[['','None'],...teamOptions()],v=>set({team:v||null})));
  if(LOOSE.includes(item.kind))fields.push(selectField('Object','kind',item.kind,LOOSE.map(each=>[each,looseTitle(each)]),v=>{ui.loose=v;set({kind:v})}));
  if('style' in item&&kind!=='ramps')fields.push(selectField(KITS.includes(item.style)?'Kitchen thing':'Material','style',item.style,styleOptions(kind==='blocks'),v=>set({style:v})));
  if('facing' in item)fields.push(selectField('Front faces','facing',item.facing,RISES.map(rise=>[rise,RISE_NAME[rise]]),v=>set({facing:v})));
  if('yaw' in item)fields.push(numberField('Facing','yaw',degrees(item.yaw),{step:5,min:-360,max:360,unit:'°'},v=>set({yaw:radians(v)})));
  // Choosing a gun writes that gun's full load (which older builds read); choosing the ammo box takes the count away (a box has none). What a pickup
  // gives is the game's rules', whatever the map says: there is nothing to set (itemNotes says what it is).
  if(kind==='pickups')fields.push(selectField('Holds','weapon',item.weapon,weaponOptions(),v=>{ui.weapon=+v;set(holding(+v))},{wide:true}));
  return fields;
}
/** Lines under an object's fields: what a sink or a bin is, what a pickup gives and how long it takes to come back (the game's rules, the same in every
 * gun mode and on every map). */
function itemNotes(found){
  if(found.kind==='sinks')return [el('p',{class:'after'},sinkNote(found.item))];
  if(found.item.kind==='bin')return [el('p',{class:'after'},'A pedal bin on the kitchen floor, beside the counter: scenery, it stops nothing. Its pedal is on the side it faces.')];
  if(found.kind!=='pickups')return [];
  const weapon=found.item.weapon,after=comeback(weapon),box=weapon===AMMO_BOX,power=POWER.includes(weapon);
  return [
    box?el('p',{class:'after'},'A box of ammunition: it fills the reserve of every gun its taker holds, but the sniper\'s and the bazooka\'s.'):
      el('p',{class:'after'},power?`A power weapon: its pickup gives ${PICKUP[weapon]} rounds, and no box tops it up.`:`Its pickup gives a magazine: ${PICKUP[weapon]} rounds.`),
    el('p',{class:'comeback'},el('strong',{},after===null?`Comes back ${seconds(POWER_BACK[0])} to ${seconds(POWER_BACK[1])} after it is taken`:`Comes back ${seconds(after)} after it is taken`),
      ' The game\'s rules, the same on every map.'),
  ];
}
/** What a sink is, and whether it has a way out. */
const sinkNote=sink=>{
  const index=doc.map.sinks.indexOf(sink),trapped=doc.analysis.issues.some(issue=>issue.code==='sink-trap'&&issue.index===index);
  return `A basin sunk ${metres(sink.depth)} m into the counter: its floor is ground like any other, its sides too tall to step up. `+
    (trapped?'It has no way out yet: draw a ramp starting inside it, up to the counter.':'A ramp from its floor to the counter is the way out.');
};
function sharedFields(items,ids){
  const fields=[],all=key=>items.every(item=>key in item),value=key=>items.every(item=>item[key]===items[0][key])?items[0][key]:null;
  const set=patch=>settle(doc.patch(ids,patch));
  // (Only ramps may start below the counter.)
  const lowest=items.every(item=>'rise' in item)?-SINK_MAX:0;
  for(const [key,label] of [['base','Sits at height'],['height','Height'],['depth','Floor below the counter']])if(all(key)){
    const v=value(key);
    fields.push(numberField(label,key,v??0,{min:key==='base'?lowest:key==='depth'?.05:0,mixed:v===null},n=>set({[key]:n})));
  }
  if(all('style')&&!items.some(item=>KITS.includes(item.style)))fields.push(selectField('Material','style',value('style'),styleOptions(false),v=>set({style:v})));
  // Several starts are given their team together.
  if(ids.every(id=>doc.find(id).kind==='starts')&&(doc.map.mode==='forts'||items.some(item=>item.team)||doc.map.zones.length)){
    const team=items.every(item=>(item.team??'')===(items[0].team??''))?items[0].team??'':null;
    fields.push(selectField('Team','team',team,[['','None'],...teamOptions()],v=>settle(doc.apply(map=>ids.reduce((next,id)=>doc.patched(next,id,{team:v||null}),map)))));
  }
  return fields;
}
function mapPanel(){
  const map=doc.map,b=map.bounds,set=patch=>settle(doc.setSettings(patch)),tool=toolById(ui.tool);
  const bound=(label,key,index)=>numberField(label,key,b[index],{step:.1,min:-LIMITS.extent,max:LIMITS.extent},v=>set({bounds:b.map((old,i)=>i===index?v:old)}));
  const nodes=[];
  if(tool.id==='pickup')nodes.push(el('h2',{},'New pickups'),el('div',{class:'fields'},
    selectField('Holds','weapon',ui.weapon,weaponOptions(),v=>{ui.weapon=+v})));
  if(tool.id==='loose')nodes.push(el('h2',{},'New loose objects'),el('div',{class:'fields'},
    selectField('Object','loose',ui.loose,LOOSE.map(each=>[each,looseTitle(each)]),v=>{ui.loose=v})));
  if(tool.id==='base')nodes.push(el('h2',{},'New bases'),el('div',{class:'fields'},
    selectField('Team','team',ui.team,teamOptions(),v=>{ui.team=v})));
  const forts=map.mode==='forts',laid=['base','field','bacon'].every(kind=>map.zones.some(zone=>zone.kind===kind))&&TEAMS.every(team=>map.zones.some(zone=>zone.kind==='base'&&zone.team===team));
  // The mode is the game's (the lobby, the page's ?mode=, the server's --mode): what a map is made for is only the mode it plays when nothing else says.
  nodes.push(el('h2',{},'Game'),
    el('div',{class:'fields'},selectField('Made for','mode',map.mode,MODES.map(mode=>[mode,MODE_NAME[mode]??title(mode)]),v=>set({mode:v}))),
    el('p',{class:'after'},map.mode==='kungfu'?'A Kung Fu map is played as Kung Fu alone.':map.mode==='titan'?'A Titan map can also be played as a siege or Free For All. Its first start is the titan\'s, in the open; the others are the small hotdogs\'. A box with a base is a den: small hotdogs walk under it, the titan cannot.':
      `The mode is chosen in the game; this is the one the map plays when none is. It can be played as a siege or Free For All${forts||laid?', and as Forts':', and as Forts once it has the Forts layout: a base for each team, a forcefield and the bacon'}.`));
  if(forts||laid)nodes.push(el('p',{},'Forts: the Mustard Eaters and the Ketchup Connoisseurs each build on their own side of the forcefield (for the game\'s build time, 150 s unless the server says otherwise), then carry the bacon home. Give every start a team, each team a base, and both sides something loose to build with.'));
  // How long a taken pickup takes to come back, and what it gives: the game's rules, the same on every map. A map that sets its own times (from before)
  // keeps them in its file, passed over; the button takes them out.
  nodes.push(el('h3',{},'Pickups'),
    el('p',{class:'after'},`A gun's pickup gives a magazine (the sniper ${PICKUP[2]} rounds, the bazooka ${PICKUP[4]}) and comes back ${seconds(RESPAWN[0])} after it is taken (the sniper and the bazooka ${seconds(POWER_BACK[0])} to ${seconds(POWER_BACK[1])}); an ammo box ${seconds(RESPAWN[1])}. The game's rules, the same on every map.`),
    map.respawn?el('div',{class:'row respawn'},el('span',{},'This map sets times of its own, which the game passes over.'),
      el('button',{type:'button',title:'Take the times out of the map',onclick:()=>set({respawn:null})},'Take them out')):null);
  nodes.push(el('h2',{},'Counter'),
    el('p',{},`${metres(b[2]-b[0])} × ${metres(b[3]-b[1])} m of worktop. Nothing can leave it.`),
    el('div',{class:'fields'},bound('Left edge (x)','b0',0),bound('Top edge (z)','b1',1),bound('Right edge (x)','b2',2),bound('Bottom edge (z)','b3',3)),
    el('div',{class:'fields'},selectField('Look in the game','theme',map.theme,THEMES.map(theme=>[theme,THEME_NAME[theme]??title(theme)]),v=>set({theme:v})),
      el('label',{class:'field wide toggle',title:'Doors and drawers down the counter\'s front and ends, and dressed wall cupboards. Scenery only.'},
        el('input',{type:'checkbox',checked:!!map.cabinets,data:{key:'cabinets'},onchange:event=>set({cabinets:event.target.checked})}),'Fitted cabinets under the counter'),
      el('label',{class:'field wide toggle',title:'The ground is the kitchen floor, not a counter: tiles, and a room whose walls stand at the edges, with a window. Scenery only.'},
        el('input',{type:'checkbox',checked:!!map.floor,data:{key:'floor'},onchange:event=>set({floor:event.target.checked})}),'Kitchen floor (a room round it)')),
    el('h3',{},'Zombie navigation'),
    el('div',{class:'fields'},
      numberField('Grid cell','cell',map.nav.cell,{step:.01,min:.01,max:.5},v=>set({nav:{cell:v}})),
      numberField('Gap kept from blocks','clear',map.nav.clearance,{step:.005,min:0,max:.2},v=>set({nav:{clearance:v}}))),
    el('h3',{},'Playing'),
    el('label',{class:'field wide'},'Game address',el('input',{type:'text',value:gameAddress(),spellcheck:false,data:{key:'game'},onchange:event=>{
      const value=event.target.value.trim();
      try{if(value)localStorage.setItem(GAME,value);else localStorage.removeItem(GAME)}catch{}
      renderInspector();
    }})),
    el('h3',{},'On this map'),
    el('dl',{class:'tally'},...KINDS.filter(kind=>kind!=='zones'||forts||map.zones.length).flatMap(kind=>[el('dt',{},title(kind==='props'?'props':kind==='starts'?'hotdog starts':kind==='spawns'?'zombie spawns':kind)),
      el('dd',{class:map[kind].length>LIMITS[kind]?'over':''},`${map[kind].length} of ${LIMITS[kind]}`)])));
  return nodes;
}
function renderInspector(){
  const panel=$('inspector'),focused=panel.contains(document.activeElement)?document.activeElement.dataset.key:null;
  const ids=doc.selection,nodes=[];
  if(!ids.length)nodes.push(...mapPanel());
  else if(ids.length===1){
    const found=doc.find(ids[0]);
    nodes.push(el('h2',{},nameOf(found)),
      el('div',{class:'fields'},...itemFields(found)),...itemNotes(found));
  }else{
    const items=ids.map(id=>doc.find(id).item),box=boxOf(items),shared=ids.length<=2000?sharedFields(items,ids):[];
    nodes.push(el('h2',{},`${ids.length} objects`),el('p',{},`Together they span ${metres(box[2]-box[0])} × ${metres(box[3]-box[1])} m.`));
    if(shared.length)nodes.push(el('div',{class:'fields'},...shared));
    const align=(label,edge)=>el('button',{type:'button',onclick:()=>settle(doc.apply(map=>alignItems(map,doc.selection,edge)))},label);
    nodes.push(el('h3',{},'Line up'),el('div',{class:'row align'},align('Left edges','minX'),align('Centres','midX'),align('Right edges','maxX'),align('Top edges','minZ'),align('Middles','midZ'),align('Bottom edges','maxZ')));
  }
  if(ids.length)nodes.push(el('div',{class:'row'},
    el('button',{type:'button',title:'Duplicate (Ctrl+D)',onclick:duplicate},'Duplicate'),
    el('button',{type:'button',title:'Delete',onclick:remove},'Delete')));
  panel.replaceChildren(...nodes);
  if(focused)panel.querySelector(`[data-key="${focused}"]`)?.focus();
}

// ---- commands ---------------------------------------------------------------------------------------------------
const nudgeStep=()=>ui.snap?ui.step:.001;
function remove(){if(doc.selection.length)settle(doc.remove(doc.selection))}
function duplicate(){
  if(!doc.selection.length)return;
  const off=Math.max(ui.step,.04);
  settle(doc.duplicate(doc.selection,off,off));
}
function paste(clip){
  let at=null;
  if(ui.inside&&ui.pointer)at=ui.snap?snapPoint(ui.pointer,ui.step):ui.pointer;
  else{const box=boxOf(clip.map(entry=>entry.item)),off=Math.max(ui.step,.04);at=[(box[0]+box[2])/2+off,(box[1]+box[3])/2+off]}
  if(settle(doc.paste(clip,at)))toast(`Pasted ${clip.length} ${clip.length===1?'object':'objects'}`);
}

// ---- pointer: selecting, moving, resizing, drawing ----------------------------------------------------------------
const snapped=(w,free)=>ui.snap&&!free?snapPoint(w,ui.step):snapPoint(w,0);
/** Snaps one coordinate of a dragged edge to nearby object edges; grid snapping is the fallback. */
function snapEdge(axis,value,edges,free){
  if(!ui.snap||free)return value;
  const hit=edgeSnap(value,edges[axis],6/view.scale);
  if(hit.guide!==null){ui.guides[axis]=hit.guide;return hit.value}
  return snapPoint([value,0],ui.step)[0];
}
function moveOffset(w,event){
  let dx=w[0]-drag.start[0],dz=w[1]-drag.start[1];
  if(event.shiftKey){if(Math.abs(dx)>=Math.abs(dz))dz=0;else dx=0}
  ui.guides={x:null,z:null};
  if(!ui.snap||event.altKey)return [dx,dz];
  const b=drag.box,reach=6/view.scale,[gx,gz]=snapPoint([b[0]+dx,b[1]+dz],ui.step);
  let nx=gx-b[0],nz=gz-b[1];
  if(drag.hasRect)for(const [axis,lo,hi,d] of [['x',b[0],b[2],dx],['z',b[1],b[3],dz]]){
    if(!d)continue;
    const a=edgeSnap(lo+d,drag.edges[axis],reach),c=edgeSnap(hi+d,drag.edges[axis],reach);
    const best=a.guide!==null&&(c.guide===null||Math.abs(a.value-lo-d)<=Math.abs(c.value-hi-d))?[a,lo]:c.guide!==null?[c,hi]:null;
    if(!best)continue;
    ui.guides[axis]=best[0].guide;
    if(axis==='x')nx=best[0].value-best[1];else nz=best[0].value-best[1];
  }
  return [dx?nx:0,dz?nz:0];
}
function ghostFor(tool,at,yaw){
  if(tool.id==='start')return {kind:'starts',p:at,yaw:yaw??.9,index:doc.map.starts.length,team:teamAt(at)};
  if(tool.id==='bacon')return {kind:'bacon',p:at};
  if(tool.id==='spawn')return {kind:'spawns',p:at};
  if(tool.id==='pickup')return {kind:'pickups',p:at,...holding(ui.weapon)};
  if(tool.id==='loose')return {kind:'props',loose:ui.loose,p:at,yaw:yaw??0};
  if(tool.id==='bin')return {kind:'props',bin:true,p:at,yaw:yaw??binYaw(at)};
  return {kind:'props',p:at,yaw:yaw??0};
}
/** Which way a new bin faces unless it is turned by hand: its pedal away from the counter, when it stands off it. */
function binYaw([x,z]){
  const b=doc.map.bounds,dx=x<b[0]?x-b[0]:x>b[2]?x-b[2]:0,dz=z<b[1]?z-b[1]:z>b[3]?z-b[3]:0;
  if(!dx&&!dz)return 0;
  return Math.abs(dx)>=Math.abs(dz)?yawTowards(Math.sign(dx),0):yawTowards(0,Math.sign(dz));
}
/** A new ramp from its foot: one whose foot is inside a sink is a wooden board propped in it, from the sink's floor up to the counter's edge (the way out). */
function rampFrom(rect,rise,foot){
  const sink=sinkAt(doc.map,foot[0],foot[1]);
  return sink?{rect,rise,base:-sink.depth,height:.012,style:'wood'}:{rect,rise};
}
function place(tool,at,yaw){
  const [x,z]=at;
  // On a Forts map a new start joins the team whose side it is put down on.
  if(tool.id==='start')return doc.add('starts',{p:at,yaw:yaw??.9,team:teamAt(at)});
  if(tool.id==='bacon')return zone({kind:'bacon',p:at});
  if(tool.id==='spawn')return doc.add('spawns',{p:at});
  if(tool.id==='pickup')return doc.add('pickups',{p:at,...holding(ui.weapon)});
  if(tool.id==='loose')return doc.add('props',{kind:ui.loose,p:at,base:surfaceAt(x,z),yaw:yaw??0});
  // A bin stands on the kitchen floor, wherever it is put: it has no height to sit at.
  if(tool.id==='bin')return doc.add('props',{kind:'bin',p:at,yaw:yaw??binYaw(at)});
  return doc.add('props',{kind:'turret',p:at,base:surfaceAt(x,z),yaw:yaw??0});
}
/** Adds a zone, or moves the one there already is: a map has one forcefield, one bacon and one base for each team. */
function zone(made){
  const old=doc.map.zones.find(each=>each.kind===made.kind&&each.team===made.team);
  if(!old)return doc.add('zones',made);
  const result=doc.patch(old.id,made);
  if(result.ok)doc.select([old.id]);
  return result;
}
function build(tool,rect,rise,foot){
  if(tool.id==='ramp')return doc.add('ramps',rampFrom(rect,rise,foot));
  if(tool.id==='sink')return doc.add('sinks',{rect,depth:SINK_DEPTH});
  if(tool.id==='base')return zone({kind:'base',team:ui.team,rect});
  if(tool.id==='field')return zone({kind:'field',rect});
  if(tool.id==='box')return doc.add('props',{kind:'box',rect,base:surfaceAt((rect[0]+rect[2])/2,(rect[1]+rect[3])/2)});
  return doc.add(tool.kind,{rect});
}
function endDrag(cancel=false){
  const was=drag;
  drag=null;ui.guides={x:null,z:null};ui.marquee=null;ui.readout=null;ui.ghost=null;
  if(was?.started&&(was.type==='move'||was.type==='resize')){
    if(cancel)doc.cancel();else settle(doc.commit());
    if(doc.selection.length>50)renderInspector();
  }
  if(was?.type==='marquee'&&cancel)doc.select(was.base);
  canvas.style.cursor=ui.space?'grab':ui.tool==='select'?'default':'crosshair';
  redraw();
}
const pointers=new Map;let pinch=null;
canvas.addEventListener('pointerdown',event=>{
  canvas.focus({preventScroll:true});
  try{canvas.setPointerCapture(event.pointerId)}catch{}
  const [px,py]=local(event),w=view.toWorld(px,py);
  pointers.set(event.pointerId,[px,py]);
  if(pointers.size===2){
    if(drag)endDrag(true);
    const [a,b]=[...pointers.values()];
    pinch={distance:Math.hypot(a[0]-b[0],a[1]-b[1])||1,mid:[(a[0]+b[0])/2,(a[1]+b[1])/2]};
    return;
  }
  if(drag)return;
  if(event.button===1||event.button===2||(event.button===0&&ui.space)){
    drag={type:'pan',px,py,cx:view.cx,cz:view.cz};canvas.style.cursor='grabbing';return;
  }
  if(event.button!==0)return;
  const tool=toolById(ui.tool);
  if(tool.id!=='select'){
    const anchor=snapped(w,event.altKey);
    drag={type:tool.point?'aim':'draw',tool,anchor,px,py,started:false,yaw:null,edges:doc.snapEdges()};
    return;
  }
  const selection=doc.selection;
  if(selection.length===1){
    const found=doc.find(selection[0]);
    if(found.item.rect){
      const handle=view.handleAt(found.item.rect,px,py);
      if(handle){drag={type:'resize',id:found.item.id,rect:found.item.rect,handle,px,py,started:false,edges:doc.snapEdges(selection)};return}
    }
  }
  const id=doc.pick(w[0],w[1],4/view.scale,{marker:view.marker()});
  if(id===null){
    drag={type:'marquee',start:w,px,py,add:event.shiftKey,base:selection,started:false};
    return;
  }
  if(event.shiftKey||event.ctrlKey||event.metaKey){doc.select([id],{toggle:true});return}
  if(!selection.includes(id))doc.select([id]);
  const ids=doc.selection,items=ids.map(each=>doc.find(each).item);
  drag={type:'move',ids,box:boxOf(items),hasRect:items.some(item=>item.rect),start:w,px,py,started:false,edges:doc.snapEdges(ids)};
});
canvas.addEventListener('pointermove',event=>{
  const [px,py]=local(event),w=view.toWorld(px,py);
  if(pointers.has(event.pointerId))pointers.set(event.pointerId,[px,py]);
  if(pinch&&pointers.size===2){
    const [a,b]=[...pointers.values()],distance=Math.hypot(a[0]-b[0],a[1]-b[1])||1,mid=[(a[0]+b[0])/2,(a[1]+b[1])/2];
    view.zoomAt(mid[0],mid[1],distance/pinch.distance);
    view.cx-=(mid[0]-pinch.mid[0])/view.scale;view.cz-=(mid[1]-pinch.mid[1])/view.scale;
    pinch={distance,mid};redraw();return;
  }
  ui.pointer=w;ui.inside=true;
  $('coords').textContent=`x ${w[0].toFixed(3)}   z ${w[1].toFixed(3)} m`;
  if(!drag){hover(px,py,w,event);return}
  const far=Math.hypot(px-drag.px,py-drag.py)>=4;
  if(drag.type==='pan'){view.cx=drag.cx-(px-drag.px)/view.scale;view.cz=drag.cz-(py-drag.py)/view.scale;redraw();return}
  if(drag.type==='move'){
    if(!drag.started){if(!far)return;drag.started=true;doc.begin();canvas.style.cursor='move'}
    const [dx,dz]=moveOffset(w,event),result=doc.preview(map=>doc.moved(map,drag.ids,dx,dz));
    ui.readout={text:result.ok?`Moved ${fmt(dx)}, ${fmt(dz)} m`:result.error,px,py};
  }else if(drag.type==='resize'){
    if(!drag.started){if(!far)return;drag.started=true;doc.begin()}
    ui.guides={x:null,z:null};
    const x=drag.handle.x?snapEdge('x',w[0],drag.edges,event.altKey):w[0],z=drag.handle.z?snapEdge('z',w[1],drag.edges,event.altKey):w[1];
    const rect=resizeRect(drag.rect,drag.handle,x,z),result=doc.preview(map=>doc.patched(map,drag.id,{rect}));
    ui.readout={text:result.ok?`${metres(rect[2]-rect[0])} × ${metres(rect[3]-rect[1])} m`:result.error,px,py};
  }else if(drag.type==='marquee'){
    if(!drag.started){if(!far)return;drag.started=true}
    const rect=[Math.min(drag.start[0],w[0]),Math.min(drag.start[1],w[1]),Math.max(drag.start[0],w[0]),Math.max(drag.start[1],w[1])];
    ui.marquee=rect;
    const hit=doc.pickAll(rect,{marker:view.marker()});
    doc.select(drag.add?[...drag.base,...hit]:hit);
  }else if(drag.type==='draw'){
    if(!drag.started){if(!far)return;drag.started=true}
    ui.guides={x:null,z:null};
    const end=[snapEdge('x',w[0],drag.edges,event.altKey),snapEdge('z',w[1],drag.edges,event.altKey)],rect=rectFromDrag(drag.anchor,end);
    drag.rect=rect;drag.rise=riseFromDrag(drag.anchor,end);
    ui.ghost={kind:drag.tool.kind,rect,base:0,height:.28,depth:SINK_DEPTH,team:ui.team,...drag.tool.extra,...(drag.tool.id==='ramp'?rampFrom(rect,drag.rise,drag.anchor):{rise:drag.rise})};
    ui.readout={text:`${metres(rect[2]-rect[0])} × ${metres(rect[3]-rect[1])} m`,px,py};
  }else if(drag.type==='aim'){
    if(drag.tool.aim&&Math.hypot(px-drag.px,py-drag.py)>=8){
      drag.started=true;drag.yaw=yawTowards(w[0]-drag.anchor[0],w[1]-drag.anchor[1]);
      ui.readout={text:`Facing ${degrees(drag.yaw)}°`,px,py};
    }
    ui.ghost=ghostFor(drag.tool,drag.anchor,drag.yaw);
  }
  redraw();
});
function hover(px,py,w,event){
  const tool=toolById(ui.tool);
  if(tool.id!=='select'){
    const at=snapped(w,event.altKey);
    ui.ghost=tool.point?ghostFor(tool,at,null):null;
    canvas.style.cursor=ui.space?'grab':'crosshair';redraw();return;
  }
  let cursor='default',id=null;
  const selection=doc.selection,single=selection.length===1?doc.find(selection[0]):null,handle=single?.item.rect?view.handleAt(single.item.rect,px,py):null;
  if(handle)cursor=handle.x&&handle.z?(handle.x===handle.z?'nwse-resize':'nesw-resize'):handle.x?'ew-resize':'ns-resize';
  else{id=doc.pick(w[0],w[1],4/view.scale,{marker:view.marker()});if(id!==null)cursor='move'}
  canvas.style.cursor=ui.space?'grab':cursor;
  if(id!==ui.hover){ui.hover=id;redraw()}
}
function release(event){
  pointers.delete(event.pointerId);
  if(pinch){if(pointers.size<2)pinch=null;return}
  if(!drag)return;
  const was=drag;
  if(was.type==='draw'){
    const [w,d]=was.tool.size,a=was.anchor;
    const rect=was.started?was.rect:snapped([a[0]-w/2,a[1]-d/2],!ui.snap).concat(snapped([a[0]+w/2,a[1]+d/2],!ui.snap));
    endDrag();settle(build(was.tool,rect,was.started?was.rise:'+x',a));
  }else if(was.type==='aim'){endDrag();settle(place(was.tool,was.anchor,was.yaw))}
  else if(was.type==='marquee'){if(!was.started&&!was.add)doc.select([]);endDrag()}
  else endDrag();
}
canvas.addEventListener('pointerup',release);
canvas.addEventListener('pointercancel',event=>{pointers.delete(event.pointerId);pinch=null;if(drag)endDrag(true)});
canvas.addEventListener('pointerleave',()=>{ui.inside=false;if(!drag){ui.hover=null;ui.ghost=null;$('coords').textContent='';redraw()}});
canvas.addEventListener('contextmenu',event=>event.preventDefault());
canvas.addEventListener('wheel',event=>{
  event.preventDefault();
  const [px,py]=local(event),delta=event.deltaMode===1?event.deltaY*32:event.deltaY;
  view.zoomAt(px,py,Math.exp(-delta*(event.ctrlKey?.01:.0016)));
  redraw();
},{passive:false});

// ---- keyboard and clipboard -------------------------------------------------------------------------------------
window.addEventListener('keydown',event=>{
  const mod=event.ctrlKey||event.metaKey,key=event.key.length===1?event.key.toLowerCase():event.key;
  if(mod&&key==='s'){event.preventDefault();download();return}
  if(mod&&key==='o'){event.preventDefault();showOpener();return}
  if(mod&&key==='Enter'){event.preventDefault();play();return}
  if(typing(event)){if(key==='Escape')event.target.blur();return}
  if(mod){
    if(key==='z'){event.preventDefault();if(event.shiftKey)doc.redo();else doc.undo()}
    else if(key==='y'){event.preventDefault();doc.redo()}
    else if(key==='a'){event.preventDefault();doc.selectAll()}
    else if(key==='d'){event.preventDefault();duplicate()}
    return;
  }
  if(event.altKey)return;
  if(key===' '){event.preventDefault();if(!ui.space){ui.space=true;if(!drag)canvas.style.cursor='grab'}return}
  if(key==='Escape'){
    if(drag)endDrag(true);else if(ui.tool!=='select')setTool('select');else doc.select([]);
    return;
  }
  if(key==='Delete'||key==='Backspace'){event.preventDefault();remove();return}
  if(key.startsWith('Arrow')){
    if(!doc.selection.length)return;
    event.preventDefault();
    const d=nudgeStep()*(event.shiftKey?10:1),[dx,dz]={ArrowLeft:[-d,0],ArrowRight:[d,0],ArrowUp:[0,-d],ArrowDown:[0,d]}[key];
    settle(doc.move(doc.selection,dx,dz));return;
  }
  if(key==='f'||key==='Home'){fit();return}
  if(key==='g'){ui.snap=!ui.snap;$('snap').checked=ui.snap;redraw();return}
  if(key==='3'){setSolid(!ui.solid);return}
  if(key==='+'||key==='='){view.zoomAt(view.w/2,view.h/2,1.25);redraw();return}
  if(key==='-'){view.zoomAt(view.w/2,view.h/2,.8);redraw();return}
  const tool=TOOLS.find(each=>each.key.toLowerCase()===key);
  if(tool)setTool(tool.id);
});
window.addEventListener('keyup',event=>{if(event.key===' '){ui.space=false;if(!drag)canvas.style.cursor=ui.tool==='select'?'default':'crosshair'}});
document.addEventListener('copy',event=>{
  if(typing(event)||!doc.selection.length)return;
  event.clipboardData.setData('text/plain',JSON.stringify({hotdogMapObjects:doc.copy(doc.selection)}));
  event.preventDefault();
  toast(`Copied ${doc.selection.length} ${doc.selection.length===1?'object':'objects'}`);
});
document.addEventListener('cut',event=>{
  if(typing(event)||!doc.selection.length)return;
  event.clipboardData.setData('text/plain',JSON.stringify({hotdogMapObjects:doc.copy(doc.selection)}));
  event.preventDefault();remove();
});
document.addEventListener('paste',event=>{
  if(typing(event))return;
  let data=null;
  try{data=JSON.parse(event.clipboardData.getData('text/plain'))}catch{}
  if(!Array.isArray(data?.hotdogMapObjects))return;
  event.preventDefault();paste(data.hotdogMapObjects);
});

// ---- wiring -----------------------------------------------------------------------------------------------------
for(const tool of TOOLS){
  if(tool.id==='block'||tool.id==='start'||tool.id==='base')$('tools').append(el('hr',tool.forts?{class:'forts'}:{}));
  const button=el('button',{type:'button',class:`tool${tool.forts?' forts':''}`,title:`${tool.label} (${tool.key})`,ariaLabel:tool.label,data:{tool:tool.id},onclick:()=>setTool(tool.id)});
  button.innerHTML=`<svg viewBox="0 0 24 24" aria-hidden="true">${tool.icon}</svg><kbd>${tool.key}</kbd>`;
  $('tools').append(button);
}
$('name').addEventListener('change',()=>{settle(doc.setSettings({name:$('name').value}));$('name').value=doc.map.name});
$('name').addEventListener('keydown',event=>{if(event.key==='Enter')event.target.blur()});
$('new').addEventListener('click',()=>{if(discardOk()){doc.load(blank());fit();toast('Started a new map')}});
$('open').addEventListener('click',showOpener);
$('browse').addEventListener('click',()=>{$('opener').close();$('file').click()});
$('opener-close').addEventListener('click',()=>$('opener').close());
$('view3d').addEventListener('click',()=>setSolid(!ui.solid));
if(solid){
  solid.onchange=redraw;
  $('solid').addEventListener('dblclick',()=>{solid.frame(frameBox());redraw()});
  new ResizeObserver(()=>{if(ui.solid){solid.resize();redraw()}}).observe($('solid'));
}else $('view3d').hidden=true;
$('file').addEventListener('change',event=>{openFile(event.target.files[0]);event.target.value=''});
$('save').addEventListener('click',download);
$('play').addEventListener('click',play);
$('undo').addEventListener('click',()=>doc.undo());
$('redo').addEventListener('click',()=>doc.redo());
$('snap').addEventListener('change',event=>{ui.snap=event.target.checked;redraw()});
$('step').addEventListener('change',event=>{ui.step=+event.target.value;redraw()});
$('overlay').addEventListener('change',event=>setOverlay(event.target.value));
$('fit').addEventListener('click',fit);
$('zoom-in').addEventListener('click',()=>{view.zoomAt(view.w/2,view.h/2,1.25);redraw()});
$('zoom-out').addEventListener('click',()=>{view.zoomAt(view.w/2,view.h/2,.8);redraw()});
const stage=$('stage');
let dragDepth=0;
stage.addEventListener('dragenter',event=>{if(event.dataTransfer?.types.includes('Files')){event.preventDefault();dragDepth++;$('drop').hidden=false}});
stage.addEventListener('dragover',event=>{if(event.dataTransfer?.types.includes('Files'))event.preventDefault()});
stage.addEventListener('dragleave',()=>{if(--dragDepth<=0){dragDepth=0;$('drop').hidden=true}});
stage.addEventListener('drop',event=>{event.preventDefault();dragDepth=0;$('drop').hidden=true;openFile(event.dataTransfer.files[0])});
window.addEventListener('beforeunload',event=>{if(doc.dirty)event.preventDefault()});
// An edit made just before leaving is still waiting on the timer: write it now.
window.addEventListener('pagehide',()=>{if(draftTimer)writeDraft()});
new ResizeObserver(()=>{view.resize();redraw()}).observe(canvas);

doc.on(event=>{
  ui.solidStale=true;
  if(event.type==='map'){syncBar();syncTools();saveDraft();scheduleProblems();sendToGame()}
  if(!(doc.dragging&&doc.selection.length>50)&&!(drag?.type==='marquee'&&doc.selection.length>50))renderInspector();
  redraw();
});

const notice=await start();
view.resize();
setTool('select');setOverlay('none');syncBar();syncTools();renderInspector();refreshProblems();
let wantSolid=window.innerWidth>=1000;
try{const stored=localStorage.getItem(SOLID);if(stored!==null)wantSolid=stored==='1'}catch{}
setSolid(wantSolid);fit();
if(notice)toast(notice);
document.fonts?.ready.then(redraw);
// For the browser console and automated checks.
window.hotdogEditor={doc,view,solid,ui,setTool,setOverlay,setSolid,fit};
