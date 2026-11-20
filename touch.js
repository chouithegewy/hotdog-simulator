// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Touch controls for phones and tablets: a stick on the left that walks (as far as it is pushed, that fast), a drag
// anywhere else that turns the view, and buttons for the keys. index.html loads this on a device with a touch screen;
// the controls show by themselves when it has no mouse (or after the first touch when it has both), the menu button
// hides them, and ?touch / ?touch=0 in the address force them on or off.
//
// The page and the game share window.hotdogTouch (src/touch.rs): the page keeps the stick (x, y), the drag since the
// game last looked (lx, ly, in mouse counts), the buttons held (down) and pressed since (taps), and in the lobby a
// cursor (cx, cy) that a drag moves; the game reads it once a frame, presses the keys the buttons stand for, and calls
// hotdogTouch.tell(state) with what this player can do now, so that only the buttons that do something are shown.
// At the card table (Fully Loaded) that is Flip and Stop (Pick and Me while it picks who gets an action card), with no
// stick: a drag anywhere else looks round the table. Dead there is no stick either: a drag takes the death camera round,
// two fingers pinched apart or together bring it nearer or take it further off (pz, the log of how many times further
// apart they are), and Fire, called Next, shows the next hotdog when there is one to watch.
//
// Everything here works from touches on the whole window, by where they land, and the overlay itself never takes a
// pointer event (only its menu does): a mouse still reaches the game and the lobby page as before.

/** The buttons, as the game numbers them (src/touch.rs). */
export const BUTTON={FIRE:1,JUMP:2,MELEE:4,CROUCH:8,PRONE:16,STIM:32,SWAP:64,PRIMARY:128,SECONDARY:256,USE:512,CARRY:1024,READY:2048,SCOPE:4096,ROUND:8192,HINTS:16384};
/** What the game says this player can do (src/touch.rs `can`). */
export const CAN={PLAY:1,ALIVE:2,BUILD:4,READY:8,SNIPER:16,SCOPED:32,CARRY:64,BACON:128,GRASP:256,PICKUP:512,FAMINE:1024,EAT:2048,PRONE:4096,CROUCH:8192,PRIMARY:16384,SECONDARY:32768,SECOND:65536,STIM:131072,STIM_WAIT:262144,OVER:524288,WATCH:1048576,LOBBY:2097152,TABLE:4194304,GO:8388608,PICK:16777216,HINTS:33554432,HINTS_ON:67108864,SWITCH:134217728,FIGHT:268435456,CHI:536870912};
/** Where a touch belongs to the page, not to the controls: its buttons and boxes, the lobby, the menus, and the sandbox's badge and tuning panel (tune.js). */
export const OWN='a,button,input,select,textarea,label,.lobby,.touch-menu,.touch-bar,.touch-show,.mission,.chat.open,.chat-form,.error,.tune';
/** The stick's reach in CSS pixels, and the share of it that does nothing (a resting thumb). */
export const REACH=56,DEAD=.12;

/** Where the stick is pushed, from the thumb's offset from the stick's middle: [x, y] at most 1 long, y down (backwards). */
export function steer(dx,dy,reach=REACH,dead=DEAD){const far=Math.hypot(dx,dy);if(!far)return [0,0];
 const push=Math.max(0,Math.min(1,far/reach)-dead)/(1-dead);return [dx/far*push,dy/far*push]}

/** The buttons shown for a state, each [button, label, class], in the order they are laid out. At the card table
 * (Fully Loaded) only two: Flip (the trigger; Pick while this player picks who gets an action card, the hotdog it looks
 * at) and Stop (the scope; Me while it picks, the one way by touch to give the card to itself), greyed while they do
 * nothing. */
export function shown(state,latched=false){const has=bit=>(state&bit)!==0,out=[];
 if(has(CAN.TABLE)&&!has(CAN.LOBBY))return [['FIRE',has(CAN.PICK)?'Pick':'Flip',has(CAN.GO)||has(CAN.PICK)?'':'dim'],['SCOPE',has(CAN.PICK)?'Me':'Stop',has(CAN.GO)||has(CAN.PICK)?'':'dim']];
 if(has(CAN.LOBBY)||!has(CAN.PLAY)){if(has(CAN.OVER))out.push(['ROUND','New round','wide']);return out}
 if(!has(CAN.ALIVE)){if(has(CAN.WATCH))out.push(['FIRE','Next','']);return out}
 // Kung Fu: hands and feet. Each button presses its Kung Fu action's key (src/touch.rs FU_KEYS); Style takes the next style.
 if(has(CAN.FIGHT)){out.push(['FIRE','Punch',''],['SCOPE','Heavy',''],['MELEE','Kick',''],['JUMP','Jump',''],['CROUCH','Crouch',has(CAN.CROUCH)||latched?'on':''],['CARRY','Block',''],['USE','Grab',''],['STIM','Dodge',''],['PRONE','Special',has(CAN.CHI)?'on':'dim'],['SWAP','Style',''])
  if(has(CAN.HINTS))out.push(['HINTS',has(CAN.HINTS_ON)?'Hide hints':'Hints','wide'+(has(CAN.HINTS_ON)?'':' dim')]);return out}
 const laden=has(CAN.CARRY)||has(CAN.BACON),locked=has(CAN.BUILD)&&!has(CAN.CARRY);
 if(!has(CAN.BACON))out.push(['FIRE',has(CAN.CARRY)?'Throw':locked?'Locked':'Fire',locked?'dim':'']);
 out.push(['JUMP',has(CAN.PRONE)?'Stand':'Jump','']);
 if(!laden)out.push(['MELEE',has(CAN.FAMINE)?'Bite':'Melee',locked?'dim':'']);
 out.push(['CROUCH','Crouch',has(CAN.CROUCH)||latched?'on':'']);
 if(!has(CAN.BACON))out.push(['PRONE','Prone',has(CAN.PRONE)?'on':'']);
 out.push(['STIM','Stim',has(CAN.STIM)?'on':has(CAN.STIM_WAIT)?'dim':'']);
 if(has(CAN.SNIPER)&&!laden)out.push(['SCOPE','Scope',has(CAN.SCOPED)?'on':'']);
 if(!laden){if(has(CAN.PRIMARY))out.push(['PRIMARY','1',has(CAN.SECOND)?'':'on']);if(has(CAN.SECONDARY))out.push(['SECONDARY','2',has(CAN.SECOND)?'on':'']);
  if(has(CAN.PRIMARY)&&has(CAN.SECONDARY))out.push(['SWAP','Swap',''])}
 if(has(CAN.PICKUP)||has(CAN.EAT)||has(CAN.SWITCH))out.push(['USE',has(CAN.PICKUP)?'Pick up':has(CAN.EAT)?'Eat':'Switch','wide']);
 if(has(CAN.CARRY)||has(CAN.GRASP))out.push(['CARRY',has(CAN.CARRY)?'Put down':'Carry','wide']);
 if(has(CAN.BUILD))out.push(['READY',has(CAN.READY)?'Ready ✓':'Ready','wide'+(has(CAN.READY)?' on':'')]);
 // The hints (H has no button of its own otherwise): in a round this player plays, to hide them or show them again.
 if(has(CAN.HINTS))out.push(['HINTS',has(CAN.HINTS_ON)?'Hide hints':'Hints','wide'+(has(CAN.HINTS_ON)?'':' dim')]);
 return out}

const KEY={FIRE:'',JUMP:'Space',MELEE:'F',CROUCH:'C',PRONE:'T',STIM:'Shift',SWAP:'Y',PRIMARY:'',SECONDARY:'',USE:'E',CARRY:'Q',READY:'R',SCOPE:'RMB',ROUND:'Enter',HINTS:''};
const remember=(key,value)=>{try{if(value===undefined)return localStorage.getItem(`hotdog.${key}`);localStorage.setItem(`hotdog.${key}`,value)}catch{}return null};

/** Puts the touch controls on the page. Returns window.hotdogTouch. */
export function start(){
 const query=new URLSearchParams(location.search),root=document.documentElement;
 const forced=query.has('touch')?query.get('touch')!=='0':null,mouse=matchMedia('(any-pointer:fine)').matches;
 // On by itself on a touch-only device; on a device with a mouse too, from the first touch; never with ?touch=0.
 let enabled=forced??(remember('touch')==='off'?false:!mouse||remember('touch')==='on'),waiting=forced==null&&!enabled&&remember('touch')!=='off';
 let look=Number(remember('touch.look'))||3;
 const pad={on:false,x:0,y:0,lx:0,ly:0,down:0,taps:0,cx:0,cy:0,pz:0,state:0,tell:state=>{pad.state=state>>>0;render()}};
 window.hotdogTouch=pad;

 const ui=document.createElement('div');ui.className='touch';ui.id='touch';ui.setAttribute('aria-hidden','true');
 ui.innerHTML=`<div class="touch-stick"><div class="touch-knob"></div></div><div class="touch-pad"></div><p class="touch-turn">Turn your phone sideways for the whole kitchen</p>`;
 const stick=ui.querySelector('.touch-stick'),knob=ui.querySelector('.touch-knob'),buttons=new Map;
 for(const name of Object.keys(BUTTON)){const b=document.createElement('div');b.className=`tb tb-${name.toLowerCase()}`;b.dataset.b=name;b.innerHTML='<b></b><i></i>';b.querySelector('i').textContent=KEY[name];b.hidden=true;ui.querySelector('.touch-pad').append(b);buttons.set(name,b)}
 // The menu: real buttons (the overlay's only ones), styled like the lobby's panel.
 const bar=document.createElement('div');bar.className='touch-bar';
 bar.innerHTML=`<button type="button" class="touch-menu-button" aria-label="Touch controls menu">☰</button><button type="button" class="touch-chat" hidden>Chat</button>`;
 const menu=document.createElement('aside');menu.className='touch-menu';menu.hidden=true;menu.setAttribute('aria-label','Touch controls');
 menu.innerHTML=`<p class="eyebrow">Touch controls</p><button type="button" class="touch-menu-chat" hidden>Chat</button><label class="touch-row"><span>Look speed</span><input type="range" min="1" max="8" step="0.25" aria-label="Look speed"><output></output></label><button type="button" class="touch-off">Hide the touch controls</button><button type="button" class="touch-close">Back to the kitchen</button><p class="hint">Left thumb walks · drag anywhere else to look · slide off Fire to aim while firing</p>`;
 const show=document.createElement('button');show.type='button';show.className='touch-show';show.textContent='Touch controls';show.hidden=true;
 document.body.append(ui,bar,menu,show);
 const slider=menu.querySelector('input'),readout=menu.querySelector('output');slider.value=String(look);readout.textContent=look.toFixed(2).replace(/\.?0+$/,'');
 slider.addEventListener('input',()=>{look=Number(slider.value);readout.textContent=slider.value;remember('touch.look',String(look))});
 const chat=bar.querySelector('.touch-chat'),menuChat=menu.querySelector('.touch-menu-chat');
 const openMenu=open=>{menu.hidden=!open;root.classList.toggle('touch-menu-open',open);if(open){release();rest()}};
 bar.querySelector('.touch-menu-button').addEventListener('click',()=>openMenu(menu.hidden));
 menu.querySelector('.touch-close').addEventListener('click',()=>openMenu(false));
 menu.querySelector('.touch-off').addEventListener('click',()=>{openMenu(false);remember('touch','off');enable(false)});
 show.addEventListener('click',()=>{remember('touch','on');enable(true)});
 for(const b of [chat,menuChat])b.addEventListener('click',()=>{openMenu(false);window.hotdogLobby?.openChat?.()});

 // ----- What is shown: the game's state decides; nothing is redrawn unless it changes.
 let mode='',latched=false,locked=false,drawn='';
 // What the game is showing ('off': a cut scene, the demo, loading), and so which set of controls, if they are on.
 const want=()=>{const s=pad.state;if(s&CAN.LOBBY)return 'lobby';if(s&CAN.TABLE)return 'table';if(s&CAN.PLAY)return (s&CAN.ALIVE)?'play':'dead';if(s&CAN.OVER)return 'over';return 'off'};
 const view=()=>!enabled||locked?'off':want();
 function render(){
  if(!(pad.state&CAN.PLAY&&pad.state&CAN.ALIVE)){latched=false;pad.down&=~BUTTON.CROUCH}
  const now=view(),list=now==='off'?[]:shown(pad.state,latched),key=now+JSON.stringify(list)+(window.hotdogLobby?'mp':'');
  pad.on=enabled&&!locked;root.classList.toggle('touch-on',pad.on);root.dataset.touch=pad.on?now:'off';root.dataset.touchView=want();
  // The menu button while they are on and there is something to play; the button that brings them back once they were hidden.
  bar.hidden=now==='off';show.hidden=enabled||locked||forced===false||want()==='off'||remember('touch')!=='off';
  chat.hidden=!window.hotdogLobby||now!=='lobby'&&now!=='table';menuChat.hidden=!window.hotdogLobby;
  if(key===drawn)return;drawn=key;
  if(now!==mode){mode=now;ui.dataset.mode=now;release();if(now!=='play')rest()}
  const on=new Set(list.map(([name])=>name));
  for(const [name,b] of buttons){b.hidden=!on.has(name)}
  for(const [name,label,kind] of list){const b=buttons.get(name);b.querySelector('b').textContent=label;b.className=`tb tb-${name.toLowerCase()}${kind?' '+kind.split(' ').join(' '):''}`;b.hidden=false}
  ui.hidden=now==='off'}
 function enable(on){enabled=on;waiting=false;if(!on){release();rest()}render()}

 // ----- Touches: each is given a part when it lands (the stick, the view, a button) and keeps it until it lifts.
 const roles=new Map;
 const interactive=el=>el instanceof Element&&!!el.closest(OWN);
 const hit=(x,y)=>{let best=null,near=Infinity;
  for(const [name,b] of buttons){if(b.hidden)continue;const r=b.getBoundingClientRect(),cx=r.left+r.width/2,cy=r.top+r.height/2,slack=10;
   if(x<r.left-slack||x>r.right+slack||y<r.top-slack||y>r.bottom+slack)continue;const d=Math.hypot(x-cx,y-cy);if(d<near){near=d;best=name}}
  return best};
 const holding=bit=>[...roles.values()].some(role=>role.bit===bit);
 function press(name){const bit=BUTTON[name];
  if(name==='CROUCH'){latched=!latched;if(latched&&pad.state&CAN.PRONE)pad.taps|=BUTTON.PRONE;render();return null}
  if(name==='PRONE'&&latched){latched=false;render()}
  pad.down|=bit;pad.taps|=bit;buttons.get(name).classList.add('down');return bit}
 function lift(role){if(role.bit==null)return;const name=Object.keys(BUTTON).find(k=>BUTTON[k]===role.bit);
  if(!holding(role.bit)){pad.down&=~role.bit;buttons.get(name)?.classList.remove('down')}}
 const syncCrouch=()=>{if(latched)pad.down|=BUTTON.CROUCH;else pad.down&=~BUTTON.CROUCH};
 // The stick rests faintly where the left thumb goes, and comes to wherever it lands.
 let base=null;
 const place=(x,y)=>{stick.style.transform='';const r=stick.getBoundingClientRect();stick.style.transform=`translate3d(${x-r.left-r.width/2}px,${y-r.top-r.height/2}px,0)`};
 function rest(){base=null;pad.x=pad.y=0;stick.classList.remove('held');stick.style.transform='';knob.style.transform=''}
 function release(){for(const role of roles.values())lift(role);roles.clear();pad.down&=BUTTON.CROUCH;pad.x=pad.y=0;for(const b of buttons.values())b.classList.remove('down')}
 const zone=(x,y)=>x<innerWidth*(innerWidth>innerHeight?.42:.5)&&y>innerHeight*(innerWidth>innerHeight?.24:.45);

 function begin(event){
  if(!enabled){if(waiting&&![...event.changedTouches].every(t=>interactive(t.target))){enable(true)}else return}
  if(locked||mode==='off')return;
  // With the menu open the controls are put away, and a touch on the kitchen closes it.
  if(!menu.hidden){if(![...event.changedTouches].every(t=>interactive(t.target))){openMenu(false);if(event.cancelable)event.preventDefault()}return}
  let ours=false;
  for(const t of event.changedTouches){if(interactive(t.target))continue;ours=true;
   const x=t.clientX,y=t.clientY,role={x,y,bit:null,kind:'look'};
   if(mode==='lobby')role.kind='cursor';
   else{const name=hit(x,y);
    if(name){role.kind=name==='FIRE'?'fire':'button';role.bit=press(name)}
    else if(mode==='play'&&zone(x,y)&&![...roles.values()].some(r=>r.kind==='stick')){role.kind='stick';const r=REACH+8;base=[Math.max(r,Math.min(x,innerWidth*.42-r)),Math.min(Math.max(y,innerHeight*.24+r),innerHeight-r)];stick.classList.add('held');place(base[0],base[1]);move(role,x,y)}
    else if(mode!=='play'&&mode!=='table'&&mode!=='dead')role.kind='none'}
   roles.set(t.identifier,role)}
  syncCrouch();
  if(ours&&event.cancelable)event.preventDefault()}
 function move(role,x,y){
  if(role.kind==='stick'){const [sx,sy]=steer(x-base[0],y-base[1]);pad.x=sx;pad.y=sy;const far=Math.hypot(x-base[0],y-base[1]),k=far>REACH?REACH/far:1;knob.style.transform=`translate3d(${(x-base[0])*k}px,${(y-base[1])*k}px,0)`}
  else if(role.kind==='look'||role.kind==='fire'){
   // Dead, two fingers on the view pinch: how much further apart (or nearer) they are, not a turn.
   const other=mode==='dead'&&role.kind==='look'&&[...roles.values()].find(r=>r!==role&&r.kind==='look');
   if(other){const was=Math.hypot(role.x-other.x,role.y-other.y),now=Math.hypot(x-other.x,y-other.y);if(was>0&&now>0)pad.pz+=Math.log(now/was)}
   else{pad.lx+=(x-role.x)*look;pad.ly+=(y-role.y)*look}}
  else if(role.kind==='cursor'){pad.cx=Math.max(-1,Math.min(1,pad.cx+(x-role.x)*2.4/innerWidth));pad.cy=Math.max(-1,Math.min(1,pad.cy+(y-role.y)*2.4/innerHeight))}
  role.x=x;role.y=y}
 function moved(event){let ours=false;for(const t of event.changedTouches){const role=roles.get(t.identifier);if(!role)continue;ours=true;move(role,t.clientX,t.clientY)}
  if(ours&&event.cancelable)event.preventDefault()}
 function end(event){let ours=false;for(const t of event.changedTouches){const role=roles.get(t.identifier);if(!role)continue;ours=true;roles.delete(t.identifier);
   // The last stretch of a drag can arrive with the lift rather than in a move of its own.
   if(event.type==='touchend'&&role.kind!=='stick')move(role,t.clientX,t.clientY);
   if(role.kind==='stick')rest();else lift(role)}
  syncCrouch();if(ours&&event.cancelable)event.preventDefault()}
 const opts={passive:false,capture:true};
 window.addEventListener('touchstart',begin,opts);window.addEventListener('touchmove',moved,opts);window.addEventListener('touchend',end,opts);window.addEventListener('touchcancel',end,opts);
 // No pinch zoom (Safari's gesture events), no long-press menu, and nothing held when the page is left.
 for(const type of ['gesturestart','gesturechange'])document.addEventListener(type,event=>event.preventDefault());
 window.addEventListener('contextmenu',event=>{if(pad.on&&!interactive(event.target))event.preventDefault()});
 window.addEventListener('blur',()=>{release();rest()});document.addEventListener('visibilitychange',()=>{if(document.hidden){release();rest()}});
 // A mouse that captures the pointer is playing: the controls step aside until it lets go.
 document.addEventListener('pointerlockchange',()=>{locked=!!document.pointerLockElement;if(locked){release();rest()}render()});
 window.addEventListener('resize',()=>{drawn='';render()});
 render();
 // For tools and checks.
 Object.assign(pad,{enable,shown:()=>shown(pad.state,latched),mode:()=>mode,latched:()=>latched});
 return pad}
