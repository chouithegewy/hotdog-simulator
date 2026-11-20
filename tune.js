// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The sandbox on the page: a SANDBOX badge, and a panel that slides in from the right (the Tune button, F2 or `) to
// tune the guns, the stim and the movement while playing. index.html loads this only with sandbox in the address.
//
// The game publishes what may be tuned on window.hotdogTune, and fires `hotdog:tune` on window whenever it changes:
//   {sandbox, ffa, editable, online, groups:[{id, title}], schema:[{key, group, field, label, unit, min, max, step, default}],
//    values:{key: value}, text:'mustard.speed=1.1 sniper.speed=9' (only the changed values: what Copy settings copies)}
// and drains window.hotdogTuneQueue every frame: 'key=value' (several, space-separated) sets values, 'reset GROUP' puts
// one group back to its defaults and 'reset' all of them.
//
// The panel keeps its pointer, touch and key events to itself: nothing on it reaches the game; touch.js leaves touches on
// .tune alone and lobby.js its keys (Enter, V).

/** The groups that are not a gun: shown in a section of their own, below the gun picked. */
export const GLOBAL=['stim','move'];
/** The keys that open and close the panel (KeyboardEvent.code). */
export const KEYS=['F2','Backquote'];
/** How often a dragged slider sends its value, at most (ms); letting go always sends it. */
export const THROTTLE=60;

/** The decimals a step has: 0.05 → 2, 1 → 0. */
export const decimals=step=>{const s=Number(step);if(!Number.isFinite(s)||s<=0)return 3;const t=String(s),e=/^(\d)(?:\.(\d+))?e-(\d+)$/.exec(t);
 if(e)return Math.min(8,Number(e[3])+(e[2]?.length??0));const i=t.indexOf('.');return i<0?0:Math.min(8,t.length-i-1)};
/** A value as the panel shows it and sends it: to the step's decimals, without trailing zeros. */
export const format=(value,step)=>{const v=Number(value);if(value===''||value==null||!Number.isFinite(v))return '';return String(Number(v.toFixed(decimals(step))))};
/** A value typed or dragged, as the game may have it: within the field's range, to its step's decimals (the default for nonsense). */
export const snap=(value,field)=>{let v=value===''||value==null?NaN:Number(value);if(!Number.isFinite(v))v=Number(field.default);
 const lo=Number(field.min),hi=Number(field.max);if(Number.isFinite(lo)&&v<lo)v=lo;if(Number.isFinite(hi)&&v>hi)v=hi;return Number(v.toFixed(decimals(field.step)))};
/** A field's current value (its default while the game has not said). */
export const valueOf=(tune,field)=>{const v=tune?.values?.[field.key];return v===''||v==null||!Number.isFinite(Number(v))?Number(field.default):Number(v)};
/** Whether a value is not the field's default, as far as the panel shows it. */
export const differs=(field,value)=>format(value,field.step)!==format(field.default,field.step);
/** The keys whose values are not their defaults, in schema order. */
export const changed=tune=>(Array.isArray(tune?.schema)?tune.schema:[]).filter(f=>f&&typeof f.key==='string'&&differs(f,valueOf(tune,f))).map(f=>f.key);
/** The command that sets values: [[field, value], …] → 'mustard.speed=1.1 sniper.speed=9'. */
export const assign=pairs=>pairs.map(([field,value])=>`${field.key}=${format(snap(value,field),field.step)}`).join(' ');
/** The groups with their fields, in the game's order (a field whose group is not listed gets one of its own); each {id, title, global, fields}. Groups with no fields are left out. */
export function sections(tune){const out=[],by=new Map;
 const add=(id,title)=>{const s={id,title:String(title??id),global:GLOBAL.includes(id),fields:[]};by.set(id,s);out.push(s);return s};
 for(const g of Array.isArray(tune?.groups)?tune.groups:[])if(g&&typeof g.id==='string'&&!by.has(g.id))add(g.id,g.title);
 for(const f of Array.isArray(tune?.schema)?tune.schema:[]){if(!f||typeof f.key!=='string')continue;const id=String(f.group??f.key.split('.')[0]);(by.get(id)??add(id,id)).fields.push(f)}
 return out.filter(s=>s.fields.length)}

const remember=(key,value)=>{try{if(value===undefined)return localStorage.getItem(`hotdog.${key}`);localStorage.setItem(`hotdog.${key}`,value)}catch{}return null};
// A text box, where a key is typing (the chat, a name), not a shortcut.
const typing=el=>el instanceof HTMLElement&&(el.tagName==='TEXTAREA'||el.isContentEditable||(el.tagName==='INPUT'&&!/^(range|checkbox|radio|button|submit|reset|color)$/.test(el.type)));
async function copy(text){
 try{if(navigator.clipboard&&window.isSecureContext){await navigator.clipboard.writeText(text);return true}}catch{}
 // A plain-http page has no clipboard API: the old way, through a selected text box.
 const area=document.createElement('textarea');area.value=text;area.setAttribute('readonly','');area.style.cssText='position:fixed;left:-9999px;top:0;opacity:0';document.body.append(area);area.select();area.setSelectionRange(0,text.length);
 let ok=false;try{ok=document.execCommand('copy')}catch{}area.remove();return ok}

const TEMPLATE=`
<div class="tune-dock"><span class="tune-badge">Sandbox</span><button type="button" class="tune-tab" aria-expanded="false" aria-controls="tune-panel">Tune<kbd>F2</kbd></button></div>
<aside class="tune-panel" id="tune-panel" aria-label="Sandbox tuning" tabindex="-1" inert>
 <header class="tune-top"><div class="tune-heading"><p class="tune-eyebrow">Sandbox · tuning</p><p class="tune-note" role="status"></p></div><button type="button" class="tune-close" aria-label="Close the tuning panel" title="Close (F2)">✕</button></header>
 <div class="tune-body"></div>
 <footer class="tune-foot"><div class="tune-actions"><button type="button" class="tune-reset-all">Reset all</button><button type="button" class="tune-copy">Copy settings</button></div>
  <input class="tune-text" type="text" readonly placeholder="no changes" aria-label="The changed settings" spellcheck="false"></footer>
</aside>`;

/** Puts the badge and the panel on the page. Returns the panel's controls (for tools and checks). */
export function start(){
 const sandboxed=new URLSearchParams(location.search).has('sandbox'),canvas=document.getElementById('game');
 const root=document.createElement('div');root.className='tune';root.id='tune';root.innerHTML=TEMPLATE;document.body.append(root);
 const $=selector=>root.querySelector(selector),dock=$('.tune-dock'),badge=$('.tune-badge'),tab=$('.tune-tab'),panel=$('.tune-panel'),body=$('.tune-body'),note=$('.tune-note'),resetAll=$('.tune-reset-all'),copyButton=$('.tune-copy'),textBox=$('.tune-text');
 let opened=false,shape='',rows=new Map,chips=new Map,dragging=null,armed=0,picked=false;
 let selected=remember('tune.group')??'';
 const tune=()=>window.hotdogTune??null;
 const queue=command=>{if(!Array.isArray(window.hotdogTuneQueue))window.hotdogTuneQueue=[];window.hotdogTuneQueue.push(command)};

 // ----- Nothing on the badge or the panel reaches the game, the lobby page's keys or the touch controls.
 for(const type of ['keydown','keyup','keypress'])root.addEventListener(type,event=>event.stopPropagation());
 for(const type of ['pointerdown','pointerup','pointermove','mousedown','mouseup','click','dblclick','wheel','contextmenu','touchstart','touchmove','touchend','touchcancel'])root.addEventListener(type,event=>event.stopPropagation(),{passive:true});
 // F2 and ` open and close it from anywhere but a text box elsewhere (the chat); Esc closes it from inside.
 window.addEventListener('keydown',event=>{const inside=root.contains(event.target);
  if(KEYS.includes(event.code)&&!event.ctrlKey&&!event.metaKey&&!event.altKey&&(inside||!typing(event.target))){event.preventDefault();event.stopPropagation();if(!event.repeat)setOpen(!opened);return}
  if(event.code==='Escape'&&opened&&inside){event.preventDefault();event.stopPropagation();setOpen(false)}},true);
 tab.addEventListener('click',()=>setOpen(!opened));$('.tune-close').addEventListener('click',()=>setOpen(false));
 // Opening lets go of the mouse; closing gives the keys back to the game (a click on it takes the mouse again).
 function setOpen(on){if(on===opened)return;opened=on;panel.classList.toggle('open',on);panel.inert=!on;document.documentElement.classList.toggle('tune-open',on);tab.setAttribute('aria-expanded',String(on));
  if(on){try{if(document.pointerLockElement)document.exitPointerLock()}catch{}const held=tune()?.weapon;if(!picked&&typeof held==='string'&&held)selected=held;sync();panel.focus({preventScroll:true})}
  else{if(root.contains(document.activeElement))document.activeElement.blur();canvas?.focus({preventScroll:true})}}

 // ----- Sending: a dragged slider at most every THROTTLE ms and always when let go; what was sent is shown until the game says otherwise.
 const sent=new Map,later=new Map,pending=new Map;
 function send(field,value,final){const key=field.key,v=snap(value,field),now=performance.now();pending.set(key,{value:v,until:now+700});clearTimeout(later.get(key));later.delete(key);
  const go=()=>{later.delete(key);sent.set(key,performance.now());queue(assign([[field,v]]))};
  const wait=THROTTLE-(now-(sent.get(key)??-Infinity));if(final||wait<=0)go();else later.set(key,setTimeout(go,wait))}
 const shown=field=>{const p=pending.get(field.key);if(p&&p.until>performance.now())return p.value;pending.delete(field.key);return valueOf(tune(),field)};
 const forget=keys=>{for(const key of keys){pending.delete(key);clearTimeout(later.get(key));later.delete(key)}};

 // ----- The panel's contents: built when what may be tuned changes, otherwise only its values are updated.
 function fieldRow(field,editable){const id=`tune-${field.key.replace(/[^\w-]/g,'-')}`,row=document.createElement('div');row.className='tune-field';row.dataset.key=field.key;
  const line=document.createElement('div'),label=document.createElement('label'),unit=document.createElement('span'),def=document.createElement('button'),box=document.createElement('input'),slider=document.createElement('input');
  line.className='tune-line';label.htmlFor=id;label.className='tune-label';label.textContent=String(field.label??field.field??field.key);
  if(field.unit){unit.className='tune-unit';unit.textContent=String(field.unit);label.append(unit)}
  def.type='button';def.className='tune-default';def.textContent=`default ${format(field.default,field.step)}`;def.title='Put back the default';
  const step=Number(field.step)>0?String(field.step):'any';
  box.type='number';box.className='tune-number';box.id=id;box.inputMode='decimal';box.step=step;box.min=String(field.min??'');box.max=String(field.max??'');box.setAttribute('aria-label',`${label.textContent}${field.unit?` (${field.unit})`:''}`);
  slider.type='range';slider.className='tune-slider';slider.min=String(field.min??0);slider.max=String(field.max??1);slider.step=step;slider.setAttribute('aria-label',label.textContent);slider.tabIndex=-1;
  box.disabled=slider.disabled=!editable;
  line.append(label,def,box);row.append(line,slider);
  const paint=value=>{row.classList.toggle('changed',differs(field,value));def.disabled=!editable||!differs(field,value);updateChips()};
  slider.addEventListener('pointerdown',()=>{dragging=field.key});slider.addEventListener('touchstart',()=>{dragging=field.key},{passive:true});
  slider.addEventListener('input',()=>{dragging=field.key;const v=snap(slider.value,field);box.value=format(v,field.step);pending.set(field.key,{value:v,until:performance.now()+700});paint(v);send(field,v,false)});
  slider.addEventListener('change',()=>{const v=snap(slider.value,field);send(field,v,true);paint(v);if(dragging===field.key)dragging=null});
  for(const type of ['pointerup','pointercancel','blur'])slider.addEventListener(type,()=>{if(dragging===field.key)dragging=null});
  const commit=()=>{if(box.value===''){box.value=format(shown(field),field.step);return}const v=snap(box.value,field);box.value=format(v,field.step);slider.value=String(v);send(field,v,true);paint(v)};
  box.addEventListener('change',commit);box.addEventListener('keydown',event=>{if(event.key==='Enter'){event.preventDefault();commit();box.select()}});box.addEventListener('focus',()=>box.select());
  def.addEventListener('click',()=>{const v=snap(field.default,field);slider.value=String(v);box.value=format(v,field.step);send(field,v,true);paint(v)});
  rows.set(field.key,{field,row,box,slider,def,paint});return row}
 function groupView(section,editable,heading){const box=document.createElement('section'),head=document.createElement('div'),title=document.createElement(heading),reset=document.createElement('button');
  box.className=`tune-group${section.global?' global':''}`;box.dataset.group=section.id;head.className='tune-group-head';title.textContent=section.title;
  reset.type='button';reset.className='tune-reset';reset.textContent=section.global?'Reset':'Reset weapon';reset.setAttribute('aria-label',`Reset ${section.title}`);reset.disabled=!editable;
  reset.addEventListener('click',()=>{forget(section.fields.map(f=>f.key));queue(`reset ${section.id}`);for(const f of section.fields){const r=rows.get(f.key);if(r){const v=snap(f.default,f);r.slider.value=String(v);r.box.value=format(v,f.step);pending.set(f.key,{value:v,until:performance.now()+700});r.paint(v)}}});
  head.append(title,reset);box.append(head);for(const f of section.fields)box.append(fieldRow(f,editable));return box}
 function build(t,list,editable){rows=new Map;chips=new Map;body.replaceChildren();
  if(!t){const wait=document.createElement('p');wait.className='tune-empty';wait.textContent='Waiting for the game: the settings show here once it has started.';body.append(wait);return}
  const guns=list.filter(s=>!s.global),rest=list.filter(s=>s.global);
  if(!guns.some(s=>s.id===selected))selected=guns[0]?.id??'';
  if(guns.length){const part=document.createElement('section'),eyebrow=document.createElement('p'),picker=document.createElement('div');part.className='tune-part';eyebrow.className='tune-eyebrow';eyebrow.textContent='Weapon';picker.className='tune-picker';picker.setAttribute('role','tablist');picker.setAttribute('aria-label','Weapon');
   for(const s of guns){const chip=document.createElement('button');chip.type='button';chip.className='tune-chip';chip.textContent=s.title;chip.dataset.group=s.id;chip.setAttribute('role','tab');chip.setAttribute('aria-selected',String(s.id===selected));
    chip.addEventListener('click',()=>{selected=s.id;picked=true;remember('tune.group',selected);sync()});picker.append(chip);chips.set(s.id,{chip,section:s})}
   part.append(eyebrow,picker);const gun=guns.find(s=>s.id===selected);if(gun)part.append(groupView(gun,editable,'h3'));body.append(part);
   // On a short screen the guns are one row that scrolls sideways: the one picked in sight.
   const on=chips.get(selected)?.chip;if(on&&picker.scrollWidth>picker.clientWidth)picker.scrollLeft=Math.max(0,on.offsetLeft-picker.offsetLeft-24)}
  if(rest.length){const part=document.createElement('section'),eyebrow=document.createElement('p');part.className='tune-part';eyebrow.className='tune-eyebrow';eyebrow.textContent='Everyone';part.append(eyebrow);for(const s of rest)part.append(groupView(s,editable,'h3'));body.append(part)}
  if(!list.length){const none=document.createElement('p');none.className='tune-empty';none.textContent='This game has nothing to tune.';body.append(none)}}
 function updateChips(){const t=tune();for(const {chip,section} of chips.values())chip.classList.toggle('changed',section.fields.some(f=>{const r=rows.get(f.key);return differs(f,r?shown(f):valueOf(t,f))}))}

 // ----- Everything from window.hotdogTune: the badge, the note, the contents and the values.
 let last=null;
 function sync(){const t=tune();last=t;const list=sections(t),editable=!!t?.editable;
  dock.hidden=t?!t.sandbox:!sandboxed;badge.textContent=t?.ffa?'Sandbox · FFA':'Sandbox';badge.title=t?.ffa?'A sandbox game of Free For All':'A sandbox game';
  note.textContent=!t?'':!editable?'This server does not accept tuning: start it with --dev and join with sandbox.':t.online?'A change reaches everyone in this game.':'Changes apply at once.';note.classList.toggle('locked',!!t&&!editable);
  const key=JSON.stringify([!!t,editable,selected,list.map(s=>[s.id,s.title,s.fields.map(f=>[f.key,f.label,f.unit,f.min,f.max,f.step,f.default])])]);
  if(key!==shape&&opened){shape=key;build(t,list,editable)}
  for(const [k,r] of rows){const v=shown(r.field);if(dragging!==k)r.slider.value=String(v);if(document.activeElement!==r.box)r.box.value=format(v,r.field.step);r.row.classList.toggle('changed',differs(r.field,v));r.def.disabled=!editable||!differs(r.field,v)}
  for(const [id,{chip}] of chips){chip.setAttribute('aria-selected',String(id===selected));chip.classList.toggle('on',id===selected)}
  updateChips();
  const text=typeof t?.text==='string'?t.text.trim():'';if(document.activeElement!==textBox)textBox.value=text;
  resetAll.disabled=!editable;copyButton.disabled=!t}
 window.addEventListener('hotdog:tune',sync);
 // In case the game published before this page was listening, or replaced the object without saying.
 setInterval(()=>{if(window.hotdogTune!==last)sync()},1000);

 // ----- The panel's buttons.
 resetAll.addEventListener('click',()=>{if(!armed){resetAll.textContent='Reset everything?';resetAll.classList.add('armed');armed=setTimeout(()=>{armed=0;resetAll.textContent='Reset all';resetAll.classList.remove('armed')},3000);return}
  clearTimeout(armed);armed=0;resetAll.textContent='Reset all';resetAll.classList.remove('armed');forget([...rows.keys()]);queue('reset');
  for(const r of rows.values()){const v=snap(r.field.default,r.field);r.slider.value=String(v);r.box.value=format(v,r.field.step);pending.set(r.field.key,{value:v,until:performance.now()+700});r.paint(v)}});
 let copied=0;const flash=text=>{copyButton.textContent=text;clearTimeout(copied);copied=setTimeout(()=>copyButton.textContent='Copy settings',1800)};
 copyButton.addEventListener('click',async()=>{const text=typeof tune()?.text==='string'?tune().text.trim():'';textBox.value=text;
  if(!text){flash('No changes');return}flash(await copy(text)?'Copied':'Copy it from the box');textBox.focus();textBox.select()});
 textBox.addEventListener('focus',()=>textBox.select());

 sync();
 const api={open:()=>setOpen(true),close:()=>setOpen(false),toggle:()=>setOpen(!opened),sync,state:()=>({opened,selected,shape:!!shape,rows:[...rows.keys()]})};
 window.hotdogTunePanel=api;return api}
