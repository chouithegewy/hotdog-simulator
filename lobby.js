// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The multiplayer lobby's side of the page: the header ("Waiting for game", players in the lobby, the next game), the next
// game (mode and map, chosen apart, which the lobby's leader chooses, or mode= and map= in the leader's address), each player's name and
// colour, the ready queue, the text chat and the voice chat. index.html loads this only in multiplayer.
// It talks to the game server over a WebSocket of its own, beside the game's: at the server's address with the path /lobby
// instead of /ws (crates/server/src/lobby.rs has the messages). The server gives that socket a token; the game hands the token over when it joins (src/network.rs reads
// window.hotdogLobby.token), which is how the server knows which hotdog is this page's.
//
// Voice is WebRTC, browser to browser, one connection to every other player (at most four), with the offers, answers and
// candidates relayed by the lobby socket. There is no relay (TURN) server, so two players are connected only when one can reach
// the other directly: on the same network, or over a private one. A microphone needs a secure page (https or localhost).
//
// It also has the graphics menus on the frame-rate line (`graphics`, at the end), which every page has, online or not: index.html
// imports this module on every page for them and starts the lobby only in multiplayer. (They live here because the game server
// hands out only these scripts of the page's own: crates/server/src/statics.rs.)

/** The five colours a player may be (its hotdog wears it on the ring round its waist, and its name tag is written in it), as the page paints them. */
export const SWATCH={red:'#e23a2e',blue:'#2f7fe6',green:'#3fae4a',yellow:'#f2c531',black:'#17130f'};
export const COLOURS=Object.keys(SWATCH);
/** Forts: what a player may ask for (the server's lobby.rs has the same list, WISHES), what the page calls each, and the teams' colours. */
export const WISHES=['mustard','ketchup','auto'],WISH_NAME={mustard:'Mustard Eaters',ketchup:'Ketchup Connoisseurs',auto:'Either'},TEAM_COLOUR=['#e3b21c','#c8281e'];
/** Forts: the ready queue in the order it is shown: each side's members under its heading (with the bots that make it up to three), then whoever is not seated yet. Each entry is {head, side, bots} or {member}. */
export const sides=roster=>{const out=[];for(const side of [0,1]){out.push({head:roster.teams?.[side]??WISH_NAME[WISHES[side]],side,bots:roster.bots?.[side]??0});for(const member of roster.members)if(member.side===side)out.push({member})}
 const rest=roster.members.filter(member=>member.side!==0&&member.side!==1);if(rest.length){out.push({head:'Arriving',side:null,bots:0});for(const member of rest)out.push({member})}return out};
const swatch=name=>SWATCH[COLOURS.includes(name)?name:'red'];
const two=n=>String(n).padStart(2,'0');
/** The numbers line under the heading: players in the lobby out of the places there are, then what is happening. */
export const header=roster=>{const members=roster?.members??[],count=`${two(members.length)} / ${two(roster?.capacity??5)}`;
 if(!members.length)return `${count} · connecting`;
 if(roster.stage==='Opening')return `${count} · the fridge opens`;
 if(roster.stage!=='Lobby')return `${count} · round in progress`;
 if(roster.countdown!=null)return `${count} · starting in ${roster.countdown}`;
 return `${count} · ${members.filter(m=>m.ready).length} ready`};
/** The game the lobby's leader chooses: its five modes, as the page names them (the server's lobby.rs PLAYS and mode_name). */
export const MODES=['siege','forts','ffa','cards','kungfu'],MODE_NAME={siege:'Siege',forts:'Forts',ffa:'Free For All',cards:'Fully Loaded',kungfu:'Kung Fu'},RANDOM='random';
/** The one map Fully Loaded is played on (the server's lobby.rs `default_map` for it): the card table stands on Counter Siege's microwave. */
export const CARDS_MAP='counter-siege';
/** Kung Fu's own map (its `mode` is kungfu, and it is played as nothing else): a bamboo steamer's lid for a ring. */
export const KUNGFU_MAP='kung-fu-dojo';
/** What each mode is, in a line under the choice. */
export const modeText=roster=>{const mode=roster?.mode,scores=roster?.scores;
 if(mode==='forts')return `Forts · three a side · first to three rounds${scores&&(scores[0]||scores[1])?` · ${scores[0]} : ${scores[1]}`:''}. Build a fort, then steal the bacon and bring it home.`;
 if(mode==='ffa')return 'Free For All · everyone fights everyone · respawn in five seconds · first to 25 kills, or the most in five minutes.';
 if(mode==='cards')return 'Cards at a little table on the microwave: flip toppings, stop before you double up. First to 100.';
 if(mode==='kungfu')return 'Kung Fu · no guns, hands and feet · seven styles · first to ten KOs, or the most in four minutes. Knock them off the ring.';
 return 'Siege · five hotdogs · one zombie wave, then the food runs out: free for all, and the last hotdog standing wins.'};
/** What the next round plays, as one line: in the lobby the leader's choice (which may be Random), once the fridge has opened the map being played. */
export const nextGame=(roster,opened=false)=>{const c=(opened?roster?.playing:null)??roster?.choice;if(!c)return '';return `${MODE_NAME[c.mode]??''} · ${c.random?'Random map':c.title}${c.random?'':c.theme?` · ${c.theme}`:''}`};
/** Whether a map of the roster's list can be played in `mode`: the modes the server says it can (each map's `modes`; the mode is the game's, not the map's).
 * A roster from a server that does not say is read as one did: the mode's own maps, every gun map for Free For All, Counter Siege for Fully Loaded. */
export const holds=(e,mode)=>Array.isArray(e?.modes)?e.modes.includes(mode):mode==='cards'?e?.name===CARDS_MAP:mode==='ffa'?e?.mode!=='kungfu':e?.mode===mode;
/** The leader's map list for a mode, chosen apart from the map: Random first (while any map can hold the mode), then the maps the mode can be played on, then
 * the rest, greyed (`disabled`) with the reason (`why`, the server's, and the gist of it in the line). A map made for another mode says so (Free For All
 * says it for every map). Of two maps with one title, one whose file is named after the title keeps it, and the other has what its file's name adds to it
 * (`counter-siege-v2`: Counter Siege (v2)), or else the file's name. Fully Loaded (Counter Siege alone, and no Random: the server would refuse any other map
 * while it is chosen) and Kung Fu (the dojo) list only their own. */
export const options=(roster,mode)=>{const maps=roster?.maps??[],cards=mode==='cards',told=maps.some(e=>Array.isArray(e.modes)),slug=t=>String(t).toLowerCase().replace(/[^a-z0-9]+/g,'-').replace(/^-+|-+$/g,'');
 const playable=maps.filter(e=>holds(e,mode)),greyed=told&&!cards&&mode!=='kungfu'?maps.filter(e=>!holds(e,mode)):[],listed=[...playable,...greyed];
 const tell=e=>{if(listed.filter(x=>x.title===e.title).length<2)return '';const base=slug(e.title);if(e.name===base)return '';return ` (${e.name.startsWith(base+'-')?e.name.slice(base.length+1):e.name})`};
 const made=e=>!cards&&e.mode!==mode?` · ${MODE_NAME[e.mode]??e.mode}`:'',line=e=>`${e.title}${tell(e)}${made(e)} · ${e.theme}`;
 const why=e=>String(e.why?.[mode]??`${e.title} cannot be played as ${MODE_NAME[mode]??mode}.`),gist=e=>{const w=why(e).split(':')[0].replace(/\.$/,'');return w.startsWith(e.title)?w.slice(e.title.length).trim():w};
 return [...cards||!playable.length?[]:[{value:RANDOM,text:'Random'}],...playable.map(e=>({value:e.name,text:line(e)})),...greyed.map(e=>({value:e.name,text:`${line(e)} · ${gist(e)}`,disabled:true,why:why(e)}))]};
/** What the address asks the leader to choose (mode= and map=), unless it is a sandbox (whose game is set up when it joins): {mode, map}, either may be null; null for nothing. */
export const asked=search=>{const query=new URLSearchParams(search);if(query.has('sandbox'))return null;
 const mode=MODES.includes(query.get('mode'))?query.get('mode'):null,map=/^[\w-]+$/.test(query.get('map')??'')?query.get('map'):null;return mode||map?{mode,map}:null};
/** The choose messages that bring the lobby's choice to what was asked: the mode first, then the map (again after a new mode, which may
 * draw another); none for what is so already. */
export const choosing=(want,choice)=>{const out=[];if(!want)return out;const mode=!!want.mode&&choice?.mode!==want.mode;
 if(mode)out.push({type:'choose',mode:want.mode});
 if(want.map&&(mode||(choice?.random?RANDOM:choice?.name)!==want.map))out.push({type:'choose',map:want.map});return out};
// The leader's mark, drawn (an emoji would need a font the page may not have): a small gold crown.
const crown=()=>{const ns='http://www.w3.org/2000/svg',svg=document.createElementNS(ns,'svg'),title=document.createElementNS(ns,'title'),path=document.createElementNS(ns,'path');
 svg.setAttribute('viewBox','0 0 16 12');svg.setAttribute('class','crown');svg.setAttribute('role','img');svg.setAttribute('aria-label','leader');title.textContent='Leader: chooses the game';
 path.setAttribute('d','M1 3.5l3.6 3.2L8 .8l3.4 5.9L15 3.5 13.6 11H2.4z');svg.append(title,path);return svg};
/** One line of the chat as an element. Everything that came over the network (names, text) is set as text content, never parsed as markup, and a colour is looked up in SWATCH, never copied into a style. */
export function line(doc,message,self){
 const row=doc.createElement('div');
 if(message.type!=='chat'){row.className=`chat-line ${message.type==='notice'?'notice':'system'}`;row.textContent=String(message.text??'');return row}
 row.className=`chat-line${message.from===self?' mine':''}`;
 const tint=doc.createElement('span'),name=doc.createElement('b'),text=doc.createElement('span');
 tint.className='swatch';tint.style.background=swatch(message.color);
 name.textContent=String(message.name??'');text.className='text';text.textContent=String(message.text??'');
 row.append(tint,name,text);return row}

/** Replays (crates/server replay.rs): the server records every match, lists them at /replays and serves each at /replays/<id>; the game watches
 * one with ?replay=<id>. A replay's length as m:ss, and what its row says: when, where, what, how long, who won and who played. */
export const replayLength=seconds=>{const s=Math.max(0,Math.floor(Number(seconds)||0));return `${Math.floor(s/60)}:${String(s%60).padStart(2,'0')}`};
export const replayText=entry=>{const mode=MODE_NAME[entry?.mode]??MODE_NAME.siege,date=String(entry?.date??'').replace('T',' ').slice(0,16);
 const who=(entry?.players??[]).map(String).join(', ');
 return {title:`${date} · ${String(entry?.map??'')} · ${mode} · ${replayLength(entry?.duration)}`,detail:`${entry?.winner?`${entry.winner} won · `:''}${who}${entry?.complete===false?' · cut short':''}`}};
/** The address that watches a replay of the server's: only an id the server could have made (letters, digits, - and _). */
export const replayHref=id=>/^[A-Za-z0-9_-]{1,96}$/.test(String(id))?`?replay=${id}`:null;
/** One replay's row as an element: everything the server sent is set as text, the links only for an id that is one. */
export function replayRow(doc,entry){const row=doc.createElement('li'),words=doc.createElement('span'),title=doc.createElement('b'),detail=doc.createElement('i'),{title:t,detail:d}=replayText(entry);
 row.className='replay';title.textContent=t;detail.textContent=d;words.append(title,detail);row.append(words);const href=replayHref(entry?.id);
 if(href){const watch=doc.createElement('a'),save=doc.createElement('a');watch.className='watch';watch.textContent='Watch';watch.href=href;save.className='save';save.textContent='Save';save.href=`replays/${entry.id}`;save.download=`${entry.id}.replay`;row.append(watch,save)}
 return row}

const TEMPLATE=`
<aside class="lobby" id="lobby" aria-label="Lobby" hidden>
 <section class="lobby-game" aria-label="Next game">
  <p class="eyebrow game-title">Next game <span class="lobby-chooser" id="lobby-chooser"></span></p>
  <div class="lobby-row"><span>Mode</span><div class="lobby-modes" id="lobby-modes" role="radiogroup" aria-label="Game mode"></div></div>
  <div class="lobby-row"><span>Map</span><div class="lobby-pick"><select id="lobby-map" aria-label="Map"></select><span class="look" id="lobby-look"></span></div></div>
  <p class="lobby-mode" id="lobby-mode"></p>
 </section>
 <p class="eyebrow">Your hotdog</p>
 <label class="lobby-row"><span>Name</span><input id="lobby-name" type="text" maxlength="48" autocomplete="off" spellcheck="false" aria-label="Your name"></label>
 <div class="lobby-row"><span>Colour</span><div class="lobby-colours" id="lobby-color" role="radiogroup" aria-label="Colour"></div></div>
 <div class="lobby-row" id="lobby-team-row" hidden><span>Team</span><div class="lobby-teams" id="lobby-team" role="radiogroup" aria-label="Team"></div></div>
 <p class="eyebrow queue-title">Ready queue</p>
 <ol class="lobby-queue" id="lobby-queue"></ol>
 <button id="lobby-ready" type="button" disabled>Loading the kitchen…</button>
 <div class="lobby-voice"><button id="lobby-mic" type="button" disabled>Microphone off</button><label><input id="lobby-ptt" type="checkbox"> Push to talk (hold V)</label></div>
 <p class="lobby-note" id="lobby-note" role="status"></p>
 <details class="lobby-replays" id="lobby-replays"><summary>Replays</summary><ol class="lobby-replay-list" id="lobby-replay-list"></ol></details>
 <p class="hint">ENTER chat · V talk · move the mouse to look around</p>
 <p class="lobby-legal">GPL-3.0-only · <a href="THIRD_PARTY_LICENSES.html" target="_blank" rel="noopener">Licences and source</a></p>
</aside>
<section class="chat" id="chat" aria-label="Chat" hidden>
 <div class="chat-voices" id="chat-voices"></div>
 <div class="chat-log" id="chat-log" role="log" aria-live="polite"></div>
 <form class="chat-form" id="chat-form" hidden><input id="chat-input" type="text" maxlength="400" autocomplete="off" placeholder="Say something · Enter sends · Esc closes" aria-label="Chat message"></form>
 <p class="chat-hint" id="chat-hint">ENTER to chat</p>
</section>`;
const remember=(key,value)=>{try{if(value===undefined)return localStorage.getItem(`hotdog.${key}`);localStorage.setItem(`hotdog.${key}`,value)}catch{}return null};
const forget=key=>{try{localStorage.removeItem(`hotdog.${key}`)}catch{}};

/** Starts the lobby on the page for the game server at `server` (ws://host:port or wss://…). Returns window.hotdogLobby. */
export function start(server){
 const $=id=>document.getElementById(id),canvas=$('game'),query=new URLSearchParams(location.search);
 document.body.insertAdjacentHTML('beforeend',TEMPLATE);document.body.classList.add('online');
 const chapter=document.querySelector('.chapter'),mission=document.querySelector('.mission'),title=chapter.querySelector('h1'),step=$('step');
 const panel=$('lobby'),nameBox=$('lobby-name'),queue=$('lobby-queue'),readyButton=$('lobby-ready'),micButton=$('lobby-mic'),ptt=$('lobby-ptt'),note=$('lobby-note');
 const chat=$('chat'),log=$('chat-log'),form=$('chat-form'),input=$('chat-input'),voices=$('chat-voices');
 let socket=null,me=null,token=null,roster=null,closed=false,notice='',everWelcome=false;
 const levels=new Map,volumes=new Map;
 let giveToken;const api={active:true,token:new Promise(resolve=>{giveToken=resolve})};window.hotdogLobby=api;
 // The game waits for the token before it joins; without a lobby it joins as a guest after eight seconds.
 setTimeout(()=>giveToken(null),8000);
 const send=message=>{if(socket?.readyState===1)socket.send(JSON.stringify(message))};
 const mine=()=>roster?.members.find(m=>m.id===me)??null;

 // ----- Name and colour: kept in the browser for next time, sent to the server, and shown as the server has them.
 // Before there was one colour there were two, lips and face: a browser that kept them takes the lips' as its colour, once, and forgets both.
 const lips=remember('lips');if(remember('color')==null&&COLOURS.includes(lips))remember('color',lips);forget('lips');forget('face');
 const wanted={name:remember('name')??'',color:remember('color'),team:query.get('team')??remember('team')};
 const pickers=COLOURS.map(colour=>{const b=document.createElement('button');b.type='button';b.className='colour';b.style.background=SWATCH[colour];b.setAttribute('role','radio');b.setAttribute('aria-label',colour);b.title=colour;b.dataset.colour=colour;
  b.addEventListener('click',()=>{wanted.color=colour;remember('color',colour);send({type:'profile',color:colour});paint()});$('lobby-color').append(b);return b});
 // Forts: the team asked for. The server makes the sides up when the match starts, three a side, and the queue shows where everyone would be now.
 const teamRow=$('lobby-team-row'),modeLine=$('lobby-mode'),teamButtons=WISHES.map(team=>{const b=document.createElement('button');b.type='button';b.className=`team ${team}`;b.textContent=WISH_NAME[team];b.setAttribute('role','radio');b.dataset.team=team;
  b.addEventListener('click',()=>{wanted.team=team;remember('team',team);send({type:'team',team});paint();canvas.focus()});$('lobby-team').append(b);return b});
 // ----- The next game: the leader picks the mode and the map (or Random), in the lobby; everyone else sees the choice. The header says it too.
 const chooser=$('lobby-chooser'),mapBox=$('lobby-map'),look=$('lobby-look'),next=document.createElement('p');next.className='lobby-next';next.id='lobby-next';chapter.append(next);
 const modeButtons=MODES.map(mode=>{const b=document.createElement('button');b.type='button';b.className=`mode ${mode}`;b.textContent=MODE_NAME[mode];b.setAttribute('role','radio');b.dataset.mode=mode;
  b.addEventListener('click',()=>{send({type:'choose',mode});canvas.focus()});$('lobby-modes').append(b);return b});
 mapBox.addEventListener('change',()=>{send({type:'choose',map:mapBox.value});canvas.focus()});mapBox.addEventListener('keydown',event=>event.stopPropagation());
 // The address may ask for a game (mode= and map=, outside a sandbox): chosen once, as soon as this page leads the lobby, before a countdown.
 let want=asked(location.search);
 const follow=()=>{if(!want||me==null||roster?.leader!==me||roster.stage!=='Lobby'||roster.countdown!=null)return;for(const message of choosing(want,roster.choice))send(message);want=null};
 let lastOptions='';
 let naming=0;const sendName=()=>{clearTimeout(naming);const name=nameBox.value.trim();if(!name||name===mine()?.name)return;wanted.name=name;remember('name',name);send({type:'profile',name})};
 nameBox.addEventListener('input',()=>{clearTimeout(naming);naming=setTimeout(sendName,600)});nameBox.addEventListener('blur',()=>{sendName();paint()});
 nameBox.addEventListener('keydown',event=>{event.stopPropagation();if(event.key==='Enter'||event.key==='Escape'){event.preventDefault();nameBox.blur();canvas.focus()}});
 nameBox.addEventListener('keyup',event=>event.stopPropagation());
 readyButton.addEventListener('click',()=>{const m=mine();if(m)send({type:'ready',ready:!m.ready});canvas.focus()});
 // ----- Replays: the server's recorded matches, listed when the list is opened (newest first), each watched with ?replay=.
 const replaysBox=$('lobby-replays'),replayList=$('lobby-replay-list');
 replaysBox.addEventListener('toggle',()=>{if(!replaysBox.open)return;replayList.replaceChildren();const wait=document.createElement('li');wait.className='replay-note';wait.textContent='Asking the server…';replayList.append(wait);
  fetch('replays').then(r=>r.ok?r.json():{replays:[]}).then(v=>{const list=Array.isArray(v?.replays)?v.replays.slice(0,30):[];replayList.replaceChildren(...list.map(e=>replayRow(document,e)));
   if(!list.length){const none=document.createElement('li');none.className='replay-note';none.textContent='No match recorded yet: every match played here is.';replayList.append(none)}}).catch(()=>{wait.textContent='The replays could not be listed.'})});

 // ----- Chat. Enter opens the box (releasing the mouse if the game had it), Enter sends, Esc closes; while it is open the game hears no keys.
 let relock=false;const typing=()=>!form.hidden;
 const say=message=>{const row=line(document,message,me),near=log.scrollHeight-log.scrollTop-log.clientHeight<40;log.append(row);while(log.children.length>80)log.firstChild.remove();
  setTimeout(()=>{row.classList.add('old');setTimeout(()=>row.classList.add('gone'),700)},14000);if(near)log.scrollTop=log.scrollHeight};
 const openChat=()=>{if(typing())return;relock=document.pointerLockElement===canvas;if(relock)document.exitPointerLock();form.hidden=false;chat.classList.add('open');input.value='';input.focus();log.scrollTop=log.scrollHeight};
 const closeChat=()=>{if(!typing())return;form.hidden=true;chat.classList.remove('open');input.blur();canvas.focus();if(relock){try{const asked=canvas.requestPointerLock();asked?.catch?.(()=>{})}catch{}}relock=false};
 form.addEventListener('submit',event=>{event.preventDefault();const text=input.value.trim();if(text)send({type:'chat',text});closeChat()});
 input.addEventListener('keydown',event=>{event.stopPropagation();if(event.key==='Escape'){event.preventDefault();closeChat()}});
 input.addEventListener('keyup',event=>event.stopPropagation());input.addEventListener('blur',()=>setTimeout(()=>{if(document.activeElement!==input)closeChat()},0));
 // (The sandbox's tuning panel, tune.js, and the graphics menus on the frame-rate line keep their keys too.)
 const field=target=>target instanceof HTMLElement&&(target.tagName==='INPUT'||target.tagName==='TEXTAREA'||target.isContentEditable||!!target.closest('.tune,#readout,.gfx-pop'));
 window.addEventListener('keydown',event=>{if(field(event.target)||event.ctrlKey||event.metaKey||event.altKey)return;
  if(event.code==='Enter'||event.code==='NumpadEnter'){event.preventDefault();event.stopPropagation();if(!event.repeat)openChat()}
  else if(event.code==='KeyV'&&!event.repeat)hold(true)},true);
 window.addEventListener('keyup',event=>{if(event.code==='KeyV')hold(false)},true);
 window.addEventListener('blur',()=>hold(false));

 // ----- Voice. A microphone if the page may have one; a connection to every other page; a level sent while speaking.
 const rtc=typeof RTCPeerConnection==='function',secure=window.isSecureContext&&!!navigator.mediaDevices?.getUserMedia;
 const stun=query.get('stun'),config={iceServers:stun?[{urls:stun}]:[]},peers=new Map;
 let mic=null,track=null,muted=remember('muted')==='1',held=false,level=0,context=null,analyser=null,samples=null,micState='none',micError='';
 ptt.checked=remember('ptt')==='1';
 const live=()=>!!track&&!muted&&(!ptt.checked||held);
 const hold=down=>{if(held===down)return;held=down;applyMic()};
 const applyMic=()=>{if(track)track.enabled=live();const state=!track?'none':live()?'live':'muted';if(state!==micState){micState=state;send({type:'mic',state})}if(state!=='live'&&level){level=0;send({type:'speaking',level:0})}paint()};
 const attach=peer=>{const sender=peer.pc.getTransceivers()[0]?.sender;if(sender&&track&&sender.track!==track)sender.replaceTrack(track).catch(()=>{})};
 async function microphone(){if(!secure||mic)return;
  try{mic=await navigator.mediaDevices.getUserMedia({audio:{echoCancellation:true,noiseSuppression:true,autoGainControl:true}});track=mic.getAudioTracks()[0];micError='';
   try{context=new (window.AudioContext||window.webkitAudioContext)();analyser=context.createAnalyser();analyser.fftSize=2048;samples=new Float32Array(analyser.fftSize);context.createMediaStreamSource(mic).connect(analyser)}catch{analyser=null}
   for(const peer of peers.values())attach(peer)}
  catch(error){mic=null;track=null;micError=error?.name==='NotAllowedError'?'The microphone is blocked: allow it for this page to talk. You can still listen.':error?.name==='NotFoundError'?'No microphone was found. You can still listen.':'The microphone could not be opened. You can still listen.'}
  applyMic()}
 micButton.addEventListener('click',()=>{if(!track){microphone();return}muted=!muted;remember('muted',muted?'1':'0');applyMic();canvas.focus()});
 ptt.addEventListener('change',()=>{remember('ptt',ptt.checked?'1':'0');applyMic();canvas.focus()});
 // The speaking level: how loud the microphone is while it is live, sent a few times a second and renewed while it lasts.
 let lastSent=0,lastLevel=0;setInterval(()=>{let now=0;
  if(analyser&&live()){if(context.state==='suspended')context.resume().catch(()=>{});analyser.getFloatTimeDomainData(samples);let sum=0;for(const s of samples)sum+=s*s;now=Math.max(0,Math.min(1,(Math.sqrt(sum/samples.length)-.012)*9))}
  // A level rises at once and falls away over a few tenths of a second, so a pause between words does not shut the mouth.
  now=Math.max(now,level*.72);level=now<.06?0:now;const t=performance.now();if((level>0&&t-lastSent>250)||Math.abs(level-lastLevel)>.15||(level===0&&lastLevel>0)){lastSent=t;lastLevel=level;send({type:'speaking',level})}
  if(me!=null)levels.set(me,level);meters()},100);
 const waitingAudio=new Set,play=audio=>audio.play().catch(()=>waitingAudio.add(audio));
 for(const type of ['pointerdown','keydown'])window.addEventListener(type,()=>{for(const audio of waitingAudio)audio.play().then(()=>waitingAudio.delete(audio)).catch(()=>{});if(context?.state==='suspended')context.resume().catch(()=>{})},true);
 const drop=id=>{const peer=peers.get(id);if(!peer)return;peers.delete(id);peer.pc.onconnectionstatechange=null;peer.pc.close();peer.audio.srcObject=null;waitingAudio.delete(peer.audio)};
 // The newer of two members (the higher id) makes the offer, so the two never offer at once.
 function connect(id,offering){drop(id);const pc=new RTCPeerConnection(config),audio=new Audio(),peer={pc,audio,pending:[],offering};peers.set(id,peer);audio.autoplay=true;audio.volume=volumes.get(id)??1;
  pc.onicecandidate=event=>{if(event.candidate)send({type:'signal',to:id,data:{candidate:event.candidate.toJSON()}})};
  pc.ontrack=event=>{audio.srcObject=event.streams[0]??new MediaStream([event.track]);play(audio)};
  pc.onconnectionstatechange=()=>{paint();if(pc.connectionState==='failed'&&peers.get(id)===peer){drop(id);setTimeout(()=>{if(offering&&!peers.has(id)&&roster?.members.some(m=>m.id===id&&m.voice))connect(id,true)},2000)}};
  if(offering){pc.addTransceiver('audio',{direction:'sendrecv'});attach(peer);pc.setLocalDescription().then(()=>send({type:'signal',to:id,data:{description:pc.localDescription.toJSON()}})).catch(()=>drop(id))}
  return peer}
 async function signalled(from,data){if(!rtc||!roster?.members.some(m=>m.id===from&&m.voice))return;let peer=peers.get(from);
  try{
   if(data.description?.type==='offer'){if(me>from)return;peer=connect(from,false);await peer.pc.setRemoteDescription(data.description);const audio=peer.pc.getTransceivers()[0];if(audio){audio.direction='sendrecv';attach(peer)}
    await peer.pc.setLocalDescription();send({type:'signal',to:from,data:{description:peer.pc.localDescription.toJSON()}});for(const c of peer.pending.splice(0))await peer.pc.addIceCandidate(c)}
   else if(data.description?.type==='answer'&&peer?.offering){await peer.pc.setRemoteDescription(data.description);for(const c of peer.pending.splice(0))await peer.pc.addIceCandidate(c)}
   else if(data.candidate&&peer){if(peer.pc.remoteDescription)await peer.pc.addIceCandidate(data.candidate);else peer.pending.push(data.candidate)}
  }catch(error){console.warn('Voice signalling failed',error)}}
 const mesh=()=>{if(!rtc||me==null||!roster)return;const others=roster.members.filter(m=>m.id!==me&&m.voice);
  for(const id of [...peers.keys()])if(!others.some(m=>m.id===id))drop(id);
  for(const m of others)if(!peers.has(m.id)&&me>m.id)connect(m.id,true)};

 // ----- What is on the page.
 const view=()=>{const m=mine();if(!roster||!m)return 'lobby';if(m.waiting||m.player==null)return 'lobby';return roster.stage==='Lobby'?'lobby':roster.stage==='Opening'?'opening':'round'};
 let lastQueue='',lastView='';
 function paint(){const m=mine(),now=view();document.body.dataset.lobby=now;mission.hidden=true;
  chapter.hidden=now==='round';chapter.classList.add('lobby-header');
  title.textContent=!roster?'Waiting for game':now==='opening'?'The fridge opens':m?.waiting?'Round in progress':'Waiting for game';
  step.textContent=notice||header(roster)+(m?.waiting?' · you join the next one':'');
  panel.hidden=now!=='lobby'||!roster;chat.hidden=!roster;chat.classList.toggle('round',now==='round');if(now!==lastView){lastView=now;log.scrollTop=log.scrollHeight}
  if(m&&document.activeElement!==nameBox&&nameBox.value!==m.name)nameBox.value=m.name;
  for(const b of pickers){const on=m?.color===b.dataset.colour;b.classList.toggle('on',on);b.setAttribute('aria-checked',String(on))}
  const forts=roster?.mode==='forts';teamRow.hidden=!forts;modeLine.hidden=!roster;document.body.dataset.mode=roster?.mode??'';
  for(const b of teamButtons){const on=(m?.team??'auto')===b.dataset.team;b.classList.toggle('on',on);b.setAttribute('aria-checked',String(on));b.disabled=!m}
  modeLine.textContent=modeText(roster);
  // The next game. Only the leader's controls work, and only in the lobby before the countdown; everyone sees what is chosen and who chooses.
  const choice=roster?.choice,leader=roster?.members.find(x=>x.id===roster.leader),leading=!!m&&roster?.leader===me,open=roster?.stage==='Lobby'&&roster?.countdown==null,can=leading&&open;
  for(const b of modeButtons){const on=choice?.mode===b.dataset.mode;b.classList.toggle('on',on);b.setAttribute('aria-checked',String(on));b.disabled=!can}
  // (The maps the chosen mode cannot be played on are greyed, with the reason as their tooltip: the mode and the map are chosen apart.)
  const listed=options(roster,choice?.mode),key=JSON.stringify(listed);if(key!==lastOptions&&document.activeElement!==mapBox){lastOptions=key;mapBox.replaceChildren(...listed.map(o=>{const option=document.createElement('option');option.value=o.value;option.textContent=o.text;if(o.disabled){option.disabled=true;option.title=o.why}return option}))}
  if(document.activeElement!==mapBox&&choice)mapBox.value=choice.random?RANDOM:choice.name;mapBox.disabled=!can;mapBox.title=can?'Choose the map':leading?'The game can only be changed in the lobby, before the countdown':'The leader chooses the map';
  look.textContent=choice?(choice.random?'drawn each round':choice.theme):'';look.dataset.theme=choice?.random?'random':choice?.theme??'';
  chooser.replaceChildren(...(leader?[crown(),document.createTextNode(leading?(open?'you choose':'you chose'):`${leader.name} chooses`)]:[document.createTextNode(roster?'the first one seated chooses':'')]));
  next.hidden=!roster||now==='round';next.replaceChildren();if(choice){const what=document.createElement('b');what.textContent=nextGame(roster,now!=='lobby'||roster.stage!=='Lobby');next.append(what);
   if(leader&&now==='lobby'){const by=document.createElement('span');by.className='by';by.append(' · ',crown(),document.createTextNode(leader.name));next.append(by)}}
  readyButton.disabled=!m||m.player==null||m.waiting||roster.stage!=='Lobby';readyButton.classList.toggle('on',!!m?.ready);
  readyButton.textContent=!m||m.player==null?'Loading the kitchen…':m.waiting?'Round in progress · you join the next one':m.ready?(roster.countdown!=null?`Starting in ${roster.countdown} · cancel`:'Ready · waiting for the others'):'Ready';
  micButton.disabled=!secure;micButton.textContent=!secure?'Microphone unavailable':!track?'Turn the microphone on':muted?'Unmute':ptt.checked?(held?'Talking…':'Hold V to talk'):'Mute';micButton.classList.toggle('on',live());
  ptt.disabled=!secure;
  note.textContent=!secure?'Voice needs the page opened over https or from localhost.'+(rtc?' You can still hear the others and use the chat.':' The chat still works.'):micError||(!rtc?'This browser has no voice chat. The chat still works.':'');
  // The ready queue, in the order people arrived, and its short form beside the chat during a round.
  const shape=JSON.stringify([roster?.members,roster?.stage,roster?.bots,roster?.leader,me,[...peers].map(([id,p])=>[id,p.pc.connectionState])]);if(shape===lastQueue)return;lastQueue=shape;
  queue.replaceChildren();voices.replaceChildren();
  for(const entry of forts?sides(roster):(roster?.members??[]).map(member=>({member}))){
   if(entry.head){const head=document.createElement('li'),dot=document.createElement('span'),name=document.createElement('b'),bots=document.createElement('span');head.className='team-head';dot.className='team-dot';dot.style.background=entry.side==null?'transparent':TEAM_COLOUR[entry.side];
    name.textContent=entry.head;bots.className='status';bots.textContent=entry.bots>0?`+ ${entry.bots} bot${entry.bots>1?'s':''}`:'';head.append(dot,name,bots);queue.append(head);continue}
   const member=entry.member,row=document.createElement('li'),tint=document.createElement('span'),name=document.createElement('b'),meter=document.createElement('span'),status=document.createElement('span');
   const playing=roster.stage!=='Lobby'&&roster.stage!=='Opening'&&member.player!=null&&!member.waiting;row.className=`member${member.ready&&!playing?' ready':''}${member.id===me?' me':''}`;row.dataset.id=member.id;tint.className='swatch';tint.style.background=swatch(member.color);tint.title=member.color??'';
   name.textContent=member.name;meter.className=`meter ${member.mic}`;meter.dataset.id=member.id;meter.title=member.mic==='live'?'microphone on':member.mic==='muted'?'muted':'no microphone';
   status.className='status';status.textContent=member.player==null?'loading':member.waiting?'next round':playing?'playing':member.ready?'READY':'not ready';row.append(tint,name);
   if(member.id===roster.leader){row.classList.add('leader');row.append(crown())}row.append(meter);
   if(member.id!==me&&member.voice&&rtc){const volume=document.createElement('input');volume.type='range';volume.min='0';volume.max='100';volume.value=String(Math.round((volumes.get(member.id)??1)*100));volume.className='volume';volume.title=`Volume of ${member.name}`;volume.setAttribute('aria-label',`Volume of ${member.name}`);
    volume.addEventListener('input',()=>{const v=Number(volume.value)/100;volumes.set(member.id,v);const peer=peers.get(member.id);if(peer)peer.audio.volume=v});volume.addEventListener('change',()=>canvas.focus());row.append(volume)}
   row.append(status);queue.append(row);
   const chip=document.createElement('span'),dot=document.createElement('span'),label=document.createElement('b');chip.className='voice-chip';dot.className=`meter ${member.mic}`;dot.dataset.id=member.id;label.textContent=member.name;chip.append(dot,label);voices.append(chip)}
  meters()}
 const peaks=new Map;
 function meters(){for(const [id,loud] of levels)peaks.set(id,Math.max(peaks.get(id)??0,loud));for(const el of document.querySelectorAll('.meter[data-id]')){const loud=levels.get(Number(el.dataset.id))??0;el.classList.toggle('speaking',loud>0);el.style.setProperty('--level',String(loud))}}

 // ----- The lobby socket.
 const fail=text=>{notice=text;paint()};
 function open(){if(closed)return;let url;
  try{const u=new URL(server);u.pathname=u.pathname.replace(/(\/ws)?\/?$/,'/lobby');url=u.href;socket=new WebSocket(url)}catch{fail(location.protocol==='https:'&&/^ws:/.test(server)?'An https page needs a wss:// server address':'The lobby address is not valid');giveToken(null);return}
  socket.onopen=()=>{notice='';send({type:'hello',...(token?{token}:{}),...(wanted.name?{name:wanted.name}:{}),...(wanted.color?{color:wanted.color}:{}),...(wanted.team?{team:wanted.team}:{})})};
  socket.onmessage=event=>{let message;try{message=JSON.parse(event.data)}catch{return}
   switch(message.type){
    case 'welcome':if(everWelcome&&message.token!==token){closed=true;socket.close();fail('The connection was lost: reload the page to rejoin');return}
     everWelcome=true;me=message.id;token=message.token;giveToken(token);if(!log.children.length)for(const old of message.history??[])say(old);if(micState!=='none')send({type:'mic',state:micState});break;
    case 'roster':roster=message;mesh();paint();follow();break;
    case 'chat':case 'system':case 'notice':say(message);break;
    case 'voice':levels.set(message.id,Number(message.level)||0);meters();break;
    case 'signal':signalled(message.from,message.data??{});break;
    case 'error':closed=true;fail(String(message.message??'The lobby refused this page'));giveToken(null);break}};
  socket.onclose=()=>{if(closed)return;if(!everWelcome)giveToken(null);fail(everWelcome?'Lobby connection lost · reconnecting':'Could not reach the lobby');setTimeout(open,2000)}}
 window.addEventListener('hotdog:stage',()=>paint());
 window.addEventListener('pagehide',()=>{closed=true;socket?.close();for(const id of [...peers.keys()])drop(id)});
 paint();open();microphone();

 // For tools and checks.
 Object.assign(api,{setReady:ready=>send({type:'ready',ready:!!ready}),setTeam:team=>send({type:'team',team:String(team)}),choose:asked=>send({type:'choose',...asked}),say:text=>send({type:'chat',text:String(text)}),profile:change=>send({type:'profile',...change}),openChat,closeChat,
  state:()=>({me,view:view(),roster,mic:micState,secure,rtc,level,levels:Object.fromEntries(levels),peaks:Object.fromEntries(peaks),lines:[...log.children].map(row=>row.textContent),peers:Object.fromEntries([...peers].map(([id,p])=>[id,{connection:p.pc.connectionState,ice:p.pc.iceConnectionState,playing:!p.audio.paused}]))}),
  stats:async()=>{const out={};for(const [id,p] of peers){const entry={connection:p.pc.connectionState,received:0,sent:0};for(const s of (await p.pc.getStats()).values()){if(s.type==='inbound-rtp'&&s.kind==='audio')entry.received+=s.bytesReceived??0;if(s.type==='outbound-rtp'&&s.kind==='audio')entry.sent+=s.bytesSent??0}out[id]=entry}return out}});
 return api}

// ----- Graphics: the menus on the frame-rate line (index.html's #readout, "60 FPS · WebGL 2 · Auto · Low · 70%"), on every
// page. Its renderer, quality and resolution each open a small menu. The renderer is a build of its own, so a new one reloads
// the page; the quality and the resolution go to the game at once (src/quality.rs reads window.hotdogGraphicsChoice and
// window.hotdogScaleChoice every frame). Each choice is kept in the browser; the address wins over it (?webgl2 or ?webgpu,
// ?quality=, ?scale=) and locks its menu. The line takes clicks in the lobby, in the fridge and before a round, never during
// one online (F8 still steps the quality there).

/** The renderers: Auto (WebGPU where the browser has a working adapter, WebGL 2 elsewhere), or either build. */
export const RENDERERS=[['auto','Auto'],['webgpu','WebGPU'],['webgl2','WebGL 2']];
/** The quality levels, as src/quality.rs names them (`Choice::key`). */
export const QUALITIES=[['auto','Auto'],['high','High'],['medium','Medium'],['low','Low'],['lowest','Lowest']];
/** The render resolution: Auto (the game's own scale) or a fixed share of the screen's pixels each way (`Resolution::key`). */
export const SCALES=[['auto','Auto'],['100','100%'],['90','90%'],['80','80%'],['70','70%'],['60','60%'],['50','50%']];
/** Where each choice is kept in local storage (the game reads hotdog-graphics and hotdog-scale when it starts, and writes them too). */
export const STORE={renderer:'hotdog-renderer',quality:'hotdog-graphics',scale:'hotdog-scale',failed:'hotdog-webgpu-failed'};
/** `STORE.failed` holds the time (ms) the WebGPU build last failed to start in this browser; for FAILED_FOR after that Auto loads WebGL 2
 * without trying again (index.html writes it, and the renderer menu clears it when WebGPU is asked for). An explicit WebGPU is still tried. */
export const FAILED_FOR=7*24*3600e3;
export const webgpuFailed=(stored,now)=>{const at=Number(stored);return !!stored&&Number.isFinite(at)&&Math.abs(now-at)<FAILED_FOR};
const BUILD_NAME={webgpu:'WebGPU',webgl2:'WebGL 2'};
/** WebGPU is offered only to secure pages: the text index.html shows when ?webgpu asks for it on any other. */
export const INSECURE='WebGPU is only available to pages opened over https:// or from localhost, and this page was opened over plain http from another machine.';

/** A quality as the game reads it (`Forced::parse`, old names included: full and lite are High, minimal is Lowest), or null. */
export function qualityKey(text){const t=String(text??'').trim().toLowerCase(),old={full:'high',lite:'high',minimal:'lowest'};
 return old[t]??(QUALITIES.some(([value])=>value===t)?t:null)}
/** A resolution as the game reads it (`Resolution::parse`): 'auto', a percentage as text ('80'), or null. `80`, `80%` and `0.8` are
 * all 80%; anything outside 25% to 100% is nothing. */
export function scaleKey(text){const t=String(text??'').trim();if(/^auto$/i.test(t))return 'auto';
 const m=/^([+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?)\s*(%?)$/i.exec(t);if(!m)return null;
 const n=Number(m[1]),share=m[2]||n>1?n/100:n;return Number.isFinite(share)&&share>=.25&&share<=1?String(Math.round(share*100)):null}
/** What the address sets, which wins over the menus: {renderer: 'webgpu', 'webgl2' or null, quality and scale: a key, or null}. */
export function addressed(search){const q=new URLSearchParams(search);
 return {renderer:q.has('webgpu')?'webgpu':q.has('webgl2')?'webgl2':null,quality:q.get('quality')?qualityKey(q.get('quality')):null,scale:q.get('scale')?scaleKey(q.get('scale')):null}}
/** Which build the page loads: `forced` by the address (or null), the `choice` in force, and whether to `probe`, that is ask the
 * browser for a WebGPU adapter and load the WebGPU build if it has one (Auto and WebGPU, where the browser has WebGPU at all). `failed`:
 * the WebGPU build failed to start here lately (webgpuFailed), which sends Auto to WebGL 2 but not a WebGPU the player chose. */
export function plan(search,saved,gpu,failed=false){const forced=addressed(search).renderer,choice=forced??(RENDERERS.some(([value])=>value===saved)?saved:'auto');
 return {forced,choice,probe:!forced&&choice!=='webgl2'&&!!gpu&&!(choice==='auto'&&failed)}}
/** Why WebGPU cannot be had on this page, or '': {why, short} for the menu. `adapter` is false when the browser was asked for one and
 * had none (null when it was not asked). */
export function webgpuBlocked({secure,gpu,adapter}){
 if(!secure)return {why:INSECURE,short:'needs https'};
 if(!gpu)return {why:'This browser has no WebGPU.',short:'not in this browser'};
 if(adapter===false)return {why:'This browser found no GPU it can use for WebGPU.',short:'no adapter'};
 return null}
/** The game's report on the frame-rate line (src/quality.rs `Detail::label`: `Auto · Low · 70%`, `Low · 70% (?quality)`): {auto, level,
 * percent}, or null before it has said anything. */
export function readLabel(text){const m=/^(Auto · )?(High|Medium|Low|Lowest) · (\d+)%/.exec(String(text??''));return m?{auto:!!m[1],level:m[2],percent:Number(m[3])}:null}

/** The three menus as the line shows them. `env`: {search, saved: {renderer, quality, scale} as stored, running ('webgpu' or 'webgl2'),
 * secure, gpu, adapter, failed (webgpuFailed), quality and scale (what the game was last told or said: window.hotdogGraphicsChoice, window.hotdogScaleChoice),
 * label (the game's report)}. Each menu: {name, title, hint, text (on the line), chosen, locked (why, or ''), note (shown under the
 * items), items: [{value, text, checked, disabled, reason, now}]}; the renderer's also has `running`. */
export function menus(env){
 const said=addressed(env.search),q=new URLSearchParams(env.search),saved=env.saved??{},label=readLabel(env.label),running=env.running==='webgpu'?'webgpu':'webgl2';
 const lock=(word,value)=>`Set by ?${word}${value?`=${value}`:''} in the address`;
 // The renderer. A saved WebGPU that this page cannot have is Auto here (the page loaded WebGL 2).
 const blocked=webgpuBlocked(env),rLocked=said.renderer?lock(said.renderer):'';
 let rChosen=said.renderer??(RENDERERS.some(([value])=>value===saved.renderer)?saved.renderer:'auto');if(!said.renderer&&rChosen==='webgpu'&&blocked)rChosen='auto';
 const failed=!!env.failed&&running==='webgl2'&&!blocked&&rChosen==='auto'&&!rLocked;
 const renderer={name:'renderer',title:'Renderer',hint:'Each is a download of its own: a change reloads the page.',text:BUILD_NAME[running],chosen:rChosen,running,locked:rLocked,
  failed,note:rLocked||(blocked?`WebGPU: ${blocked.why}`:failed?'WebGPU did not start in this browser lately, so Auto is using WebGL 2. Choose WebGPU to try it again.':''),
  items:RENDERERS.map(([value,text])=>{const off=value==='webgpu'&&!!blocked;return {value,text,checked:value===rChosen,disabled:!!rLocked||off,reason:off?blocked.short:'',now:value==='auto'&&rChosen==='auto'?BUILD_NAME[running]:''}})};
 // The quality: on the line as the game reports it.
 const qChosen=said.quality??qualityKey(env.quality)??qualityKey(saved.quality)??'auto',qLocked=said.quality?lock('quality',q.get('quality')):'',qName=Object.fromEntries(QUALITIES);
 const quality={name:'quality',title:'Quality',hint:'Applies at once. Auto follows the frame rate; F8 steps through them in a round.',text:label?`${label.auto?'Auto · ':''}${label.level}`:qName[qChosen],chosen:qChosen,locked:qLocked,note:qLocked,
  items:QUALITIES.map(([value,text])=>({value,text,checked:value===qChosen,disabled:!!qLocked,reason:'',now:value==='auto'&&qChosen==='auto'&&label?label.level:''}))};
 // The resolution: the percentage drawn, as the game reports it.
 const sChosen=said.scale??scaleKey(env.scale)??scaleKey(saved.scale)??'auto',sLocked=said.scale?lock('scale',q.get('scale')):'';
 const scale={name:'scale',title:'Resolution',hint:'The share of the screen’s pixels drawn, each way. Auto lets the game choose.',text:label?`${label.percent}%`:sChosen==='auto'?'--%':`${sChosen}%`,chosen:sChosen,locked:sLocked,note:sLocked,
  items:SCALES.map(([value,text])=>({value,text,checked:value===sChosen,disabled:!!sLocked,reason:'',now:value==='auto'&&sChosen==='auto'&&label?`${label.percent}%`:''}))};
 return {renderer,quality,scale}}

/** The address a new renderer reloads to: this one, every part kept as it was, without ?webgl2 or ?webgpu (the saved choice takes their place). */
export function rendererAddress(href){const url=new URL(href);
 const kept=url.search.slice(1).split('&').filter(part=>{if(!part)return false;let key=part.split('=')[0];try{key=decodeURIComponent(key.replace(/\+/g,' '))}catch{}return key!=='webgl2'&&key!=='webgpu'});
 return url.pathname+(kept.length?`?${kept.join('&')}`:'')+url.hash}
/** What choosing `value` in menu `name` does: `save` [key, value] for local storage; for the quality and the resolution, `tell`
 * [global, value] for the game; for a renderer, `reload` (the address) unless the build running is the one asked for or nothing
 * changed. Null when it cannot be chosen: the menu is locked or the item disabled. */
export function choose(model,name,value,href){const menu=model?.[name],item=menu?.items.find(i=>i.value===value);if(!item||item.disabled||menu.locked)return null;
 if(name==='renderer'){const retry=!!menu.failed&&value!=='webgl2'; // a new try at WebGPU: the page reloads, and the remembered failure goes
  return {save:[STORE.renderer,value],reload:(value===menu.chosen||value===menu.running)&&!retry?null:rendererAddress(href),...(retry?{forget:STORE.failed}:{})}}
 return {save:[STORE[name],value],tell:[name==='quality'?'hotdogGraphicsChoice':'hotdogScaleChoice',value]}}
/** Whether the line takes clicks. Online, not during a round (the lobby says when: body[data-lobby=round]); offline, where there is no
 * lobby and the round starts as the page loads, wherever the links to the other modes are clickable too, so not while the touch
 * controls are out. Either way, yes while the touch controls' menu is open (it puts the controls away). */
export function clickable({online,view,touch,menu}){if(menu)return true;if(online)return view!=='round';return !['play','dead','table'].includes(touch)}

/** Puts the menus on the frame-rate line. `env`: {secure, gpu, adapter} as the page found them (the build running is read from
 * html[data-renderer]). Returns window.hotdogGraphicsMenus, for tools and checks. */
export function graphics(env){
 const $=id=>document.getElementById(id),readout=$('readout'),fps=$('fps'),canvas=$('game'),root=document.documentElement,body=document.body;
 const stored=key=>{try{return localStorage.getItem(key)||''}catch{return ''}},store=(key,value)=>{try{localStorage.setItem(key,value)}catch{}};
 const saved={renderer:stored(STORE.renderer),quality:stored(STORE.quality),scale:stored(STORE.scale)};
 // What the game starts from; it reports back what it chose (src/quality.rs `graphics_show`).
 window.hotdogGraphicsChoice=qualityKey(saved.quality)??'auto';window.hotdogScaleChoice=scaleKey(saved.scale)??'auto';
 const NAMES=['renderer','quality','scale'],triggers={},parts=document.createElement('span');parts.className='gfx-parts';
 for(const name of NAMES){const dot=document.createElement('span'),b=document.createElement('button');dot.className='gfx-dot';dot.textContent=' · ';dot.setAttribute('aria-hidden','true');
  b.type='button';b.className='gfx-part';b.id=`gfx-${name}`;b.dataset.menu=name;b.tabIndex=-1;b.setAttribute('aria-haspopup','menu');b.setAttribute('aria-expanded','false');b.setAttribute('aria-controls','gfx-menu');
  parts.append(dot,b);triggers[name]=b}
 readout.append(parts);
 const pop=document.createElement('div');pop.className='gfx-pop';pop.hidden=true;
 pop.innerHTML='<p class="gfx-head" id="gfx-head"></p><p class="gfx-hint"></p><div class="gfx-items" id="gfx-menu" role="menu" aria-labelledby="gfx-head"></div><p class="gfx-note" id="gfx-note"></p>';
 body.append(pop);
 const head=pop.querySelector('.gfx-head'),hint=pop.querySelector('.gfx-hint'),list=pop.querySelector('.gfx-items'),note=pop.querySelector('.gfx-note');
 let model=null,open=null,live=false;
 const read=()=>({search:location.search,saved,running:root.dataset.renderer,secure:env.secure,gpu:env.gpu,adapter:env.adapter,failed:webgpuFailed(stored(STORE.failed),Date.now()),quality:window.hotdogGraphicsChoice,scale:window.hotdogScaleChoice,label:fps.dataset.quality||''});
 const items=()=>[...list.querySelectorAll('.gfx-item')];
 function render(){model=menus(read());
  for(const name of NAMES){const m=model[name],b=triggers[name];b.textContent=m.text;b.classList.toggle('locked',!!m.locked);b.title=m.locked||`${m.title}: ${m.hint}`;b.setAttribute('aria-label',`${m.title}: ${m.text}${m.locked?`. ${m.locked}`:''}`)}
  if(open)fill()}
 // The open menu's items, rebuilt; the focus stays on the item it was on.
 function fill(){const m=model[open],had=list.contains(document.activeElement)?document.activeElement.dataset.value:null;pop.dataset.menu=open;
  head.textContent=m.title;hint.textContent=m.hint;note.textContent=m.note;note.hidden=!m.note;if(m.note)list.setAttribute('aria-describedby','gfx-note');else list.removeAttribute('aria-describedby');
  list.replaceChildren(...m.items.map(item=>{const b=document.createElement('button'),text=document.createElement('span');b.type='button';b.className='gfx-item';b.tabIndex=-1;b.dataset.value=item.value;
   b.setAttribute('role','menuitemradio');b.setAttribute('aria-checked',String(item.checked));if(item.disabled)b.setAttribute('aria-disabled','true');
   text.textContent=item.text;b.append(text);if(item.reason||item.now){const small=document.createElement('small');small.textContent=item.reason||`now ${item.now}`;b.append(small)}return b}));
  if(had!=null)items().find(b=>b.dataset.value===had)?.focus()}
 // Above its part of the line, right-aligned with it, or below it where there is more room there; never off the screen.
 function place(){const r=triggers[open].getBoundingClientRect(),gap=8,edge=8;pop.style.maxHeight='';pop.style.left='0px';pop.style.top='0px';
  const w=pop.offsetWidth,h=pop.offsetHeight,above=r.top-gap-edge,below=innerHeight-r.bottom-gap-edge,up=h<=above||above>=below,room=Math.max(80,up?above:below);
  if(h>room)pop.style.maxHeight=`${room}px`;const tall=Math.min(h,room);
  pop.style.top=`${Math.round(up?r.top-gap-tall:r.bottom+gap)}px`;pop.style.left=`${Math.round(Math.max(edge,Math.min(r.right-w,innerWidth-edge-w)))}px`}
 function openMenu(name,focus='checked'){if(open)close(false);open=name;triggers[name].setAttribute('aria-expanded','true');fill();pop.hidden=false;place();
  const all=items(),at=focus==='last'?all.length-1:Math.max(0,all.findIndex(b=>b.getAttribute('aria-checked')==='true'));all[at]?.focus()}
 // `refocus`: true puts the keyboard back on the menu's part of the line, 'game' gives it to the game, false leaves it.
 function close(refocus){if(!open)return;const b=triggers[open];b.setAttribute('aria-expanded','false');open=null;pop.hidden=true;delete pop.dataset.menu;
  if(refocus===true)b.focus();else if(refocus==='game')canvas?.focus()}
 function pick(value,pointer){const plan=choose(model,open,value,location.href);if(!plan)return;
  store(...plan.save);saved[open]=value;if(plan.forget)try{localStorage.removeItem(plan.forget)}catch{}
  if(plan.reload){close(false);history.replaceState(history.state,'',plan.reload);location.reload();return}
  if(plan.tell)window[plan.tell[0]]=plan.tell[1];
  close(pointer?'game':true);render()}
 for(const name of NAMES){const b=triggers[name];
  b.addEventListener('click',()=>{if(open===name)close(true);else openMenu(name)});
  // Enter and Space open it as a click does; the arrows open it too, and Left and Right go along the line. No key reaches the game.
  b.addEventListener('keydown',event=>{const at=NAMES.indexOf(name);
   if(event.key==='ArrowDown'||event.key==='ArrowUp'){event.preventDefault();openMenu(name,event.key==='ArrowUp'?'last':'checked')}
   else if(event.key==='ArrowLeft'||event.key==='ArrowRight'){event.preventDefault();triggers[NAMES[(at+(event.key==='ArrowLeft'?NAMES.length-1:1))%NAMES.length]].focus()}
   else if(event.key!=='Enter'&&event.key!==' '&&event.key!=='Tab')return;
   event.stopPropagation()})}
 list.addEventListener('click',event=>{const b=event.target.closest('.gfx-item');if(b)pick(b.dataset.value,event.detail>0)});
 // Arrows move through the items (the disabled ones too, so their reasons can be read), Home and End to the ends, Left and Right to the
 // next menu along the line, Esc and Tab close it. Enter and Space choose, as a click.
 pop.addEventListener('keydown',event=>{event.stopPropagation();const all=items(),at=all.indexOf(document.activeElement),n=all.length;
  const go=i=>{event.preventDefault();all[(i+n)%n]?.focus()};
  switch(event.key){
   case 'ArrowDown':go(at+1);break;case 'ArrowUp':go(at<0?n-1:at-1);break;case 'Home':go(0);break;case 'End':go(n-1);break;
   case 'ArrowLeft':case 'ArrowRight':{event.preventDefault();const i=NAMES.indexOf(open);openMenu(NAMES[(i+(event.key==='ArrowLeft'?NAMES.length-1:1))%NAMES.length]);break}
   case 'Escape':case 'Tab':event.preventDefault();close(true);break}});
 pop.addEventListener('keyup',event=>event.stopPropagation());
 // A click anywhere else closes it (and goes where it was aimed); so do the window changing size and the game taking the pointer.
 document.addEventListener('pointerdown',event=>{if(open&&!pop.contains(event.target)&&!readout.contains(event.target))close(false)},true);
 window.addEventListener('resize',()=>close(false));
 document.addEventListener('pointerlockchange',()=>{if(document.pointerLockElement)close(false)});
 // Clickable or not, as the lobby, the round and the touch controls go.
 function watch(){const now=clickable({online:body.classList.contains('online'),view:body.dataset.lobby,touch:root.dataset.touch,menu:root.classList.contains('touch-menu-open')});
  if(now===live)return;live=now;readout.classList.toggle('gfx-live',now);for(const b of Object.values(triggers))b.tabIndex=now?0:-1;
  if(!now){const held=readout.contains(document.activeElement)||pop.contains(document.activeElement);close(false);if(held)canvas?.focus()}}
 new MutationObserver(watch).observe(body,{attributes:true,attributeFilter:['class','data-lobby']});
 new MutationObserver(watch).observe(root,{attributes:true,attributeFilter:['class','data-touch']});
 // What the game reports, and the build when the page falls back to WebGL 2.
 new MutationObserver(render).observe(fps,{attributes:true,attributeFilter:['data-quality']});
 new MutationObserver(render).observe(root,{attributes:true,attributeFilter:['data-renderer']});
 window.addEventListener('hotdog:graphics',render);
 render();watch();
 const api={render,open:name=>openMenu(name),close:()=>close(false),state:()=>({open,live,model:menus(read())})};window.hotdogGraphicsMenus=api;return api}
