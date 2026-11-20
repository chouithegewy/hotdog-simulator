// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The page's side of the lobby (lobby.js): what it shows of what the server sends. The server's side is tested in Rust (crates/server).
import test from 'node:test';
import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
import {line,SWATCH,COLOURS,header,sides,WISHES,nextGame,options,holds,MODES,MODE_NAME,modeText,asked,choosing,CARDS_MAP,KUNGFU_MAP,TITAN_MAP,ownMap,TITAN_PICKS,titanOptions,titanValue,titanText,replayText,replayRow,replayHref,replayLength,trailNote,trailLine,TRAIL,replayStart,lightAddress,fullAddress,replayTrouble,REPLAY_GUARD,REPLAY_RECENT} from '../lobby.js';

test('the page shows names and chat as text, never as markup',()=>{
 // A stand-in for the document: elements only take text, classes and children, as the real ones are used.
 const made=[],element=tag=>{const e={tag,className:'',children:[],style:{},dataset:{},append(...kids){e.children.push(...kids)},set innerHTML(_){throw new Error('markup must never be assigned')}};made.push(e);return e},doc={createElement:element};
 const evil='<img src=x onerror=alert(1)>',row=line(doc,{type:'chat',from:2,name:evil,color:'blue',text:`${evil}<script>alert(2)</script>`},1);
 const flat=e=>[e,...e.children.flatMap(flat)],texts=flat(row).map(e=>e.textContent).filter(Boolean);
 assert.deepEqual(texts,[evil,`${evil}<script>alert(2)</script>`],'the name and the line, as text content');
 assert.ok(made.every(e=>['div','span','b','i'].includes(e.tag)),'no element is made from what was sent');
 assert.deepEqual(row.children.filter(e=>e.className.includes('swatch')).map(e=>e.style.background),[SWATCH.blue],'one swatch, of the colour');
 // A colour that is not one of the five (a server gone wrong) never reaches a style.
 const odd=line(doc,{type:'chat',from:2,name:'x',color:'blue;background:url(//evil)',text:'y'},1);assert.equal(odd.children.find(e=>e.className.includes('swatch')).style.background,SWATCH.red);
 assert.equal(line(doc,{type:'system',text:`${evil} joined`},1).textContent,`${evil} joined`);
 // The header counts the players in the lobby out of the places there are.
 const members=[{ready:true,player:1},{ready:false,player:2}];
 assert.equal(header({stage:'Lobby',countdown:null,capacity:5,members}),'02 / 05 · 1 ready');assert.equal(header({stage:'Lobby',countdown:3,capacity:5,members}),'02 / 05 · starting in 3');
 assert.equal(header({stage:'Opening',countdown:null,capacity:5,members}),'02 / 05 · the fridge opens');assert.equal(header({stage:'Lobby',countdown:null,capacity:5,members:[]}),'00 / 05 · connecting');
 // The five colours are the server's (crates/server/src/game.rs COLOURS).
 assert.deepEqual(COLOURS,['red','blue','green','yellow','black']);
});

test('Forts: the page groups the ready queue by side, with the bots that make each up to three (and those not seated yet under their own heading), and asks for the teams the server knows',()=>{
 // The server's lobby.rs WISHES, in another order.
 assert.deepEqual([...WISHES].sort(),['auto','ketchup','mustard']);
 const roster={teams:['Mustard Eaters','Ketchup Connoisseurs'],bots:[2,0],members:[{name:'Ann',side:0},{name:'Ben',side:1},{name:'Cy',side:1},{name:'Dee',side:1},{name:'Eve',side:null}]};
 assert.deepEqual(sides(roster).map(e=>e.head?`${e.head} +${e.bots}`:e.member.name),['Mustard Eaters +2','Ann','Ketchup Connoisseurs +0','Ben','Cy','Dee','Arriving +0','Eve']);
});

test('the page says what is next (Random until the fridge opens, then the map drawn) and lists Random first, then the mode\'s maps',()=>{
 const maps=[{name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'},{name:'counter-siege-v2',title:'Counter Siege',mode:'siege',theme:'kitchen'},{name:'horde-night',title:'Horde Night',mode:'siege',theme:'night'},{name:'pillow-fort',title:'Pillow Fort',mode:'forts',theme:'kitchen'}];
 const roster={choice:{name:'random',title:'Random',mode:'forts',theme:'',random:true},playing:{name:'pillow-fort',title:'Pillow Fort',mode:'forts',theme:'kitchen'},maps};
 assert.equal(nextGame(roster),'Forts · Random map');assert.equal(nextGame(roster,true),'Forts · Pillow Fort · kitchen');assert.equal(nextGame({...roster,choice:maps[2]}),'Siege · Horde Night · night');assert.equal(nextGame(null),'');
 assert.deepEqual(options(roster,'siege').map(o=>[o.value,o.text]),[['random','Random'],['counter-siege','Counter Siege · kitchen'],['counter-siege-v2','Counter Siege (v2) · kitchen'],['horde-night','Horde Night · night']]);
 assert.deepEqual(options(roster,'forts').map(o=>o.value),['random','pillow-fort']);assert.deepEqual(MODES,['siege','forts','ffa','cards','kungfu','titan']);
});

test('Free For All: a third mode, played on any map, each listed with the mode it was made for',()=>{
 const maps=[{name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'},{name:'counter-siege-v2',title:'Counter Siege',mode:'siege',theme:'kitchen'},{name:'pillow-fort',title:'Pillow Fort',mode:'forts',theme:'kitchen'}];
 assert.deepEqual(options({maps},'ffa').map(o=>[o.value,o.text]),[['random','Random'],['counter-siege','Counter Siege · Siege · kitchen'],['counter-siege-v2','Counter Siege (v2) · Siege · kitchen'],['pillow-fort','Pillow Fort · Forts · kitchen']]);
 assert.equal(MODE_NAME.ffa,'Free For All');
 assert.equal(nextGame({choice:{name:'pillow-fort',title:'Pillow Fort',mode:'ffa',theme:'kitchen'}}),'Free For All · Pillow Fort · kitchen');
 assert.match(modeText({mode:'ffa'}),/^Free For All · everyone fights everyone/);assert.match(modeText({mode:'forts',scores:[1,2]}),/^Forts .* · 1 : 2\./);assert.match(modeText({mode:'siege'}),/^Siege/);
});

test('Fully Loaded: a fourth mode, played only on Counter Siege, so its map list is that one map and no Random',()=>{
 const maps=[{name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'},{name:'counter-siege-v2',title:'Counter Siege',mode:'siege',theme:'kitchen'},{name:'horde-night',title:'Horde Night',mode:'siege',theme:'night'},{name:'pillow-fort',title:'Pillow Fort',mode:'forts',theme:'kitchen'}];
 assert.deepEqual(MODES.slice(0,4),['siege','forts','ffa','cards'],'after Free For All');assert.equal(MODE_NAME.cards,'Fully Loaded');assert.equal(CARDS_MAP,'counter-siege','the map the server plays it on (lobby.rs default_map)');
 assert.equal(modeText({mode:'cards'}),'Cards at a little table on the microwave: flip toppings, stop before you double up. First to 100.');
 assert.deepEqual(options({maps},'cards').map(o=>[o.value,o.text]),[['counter-siege','Counter Siege · kitchen']],'no Random, no other map, no (v2) to tell it from');
 assert.deepEqual(options({maps:maps.slice(2)},'cards'),[],'without Counter Siege there is nothing to choose');assert.deepEqual(options(null,'cards'),[]);
 // What is next, as the server names the entry (Counter Siege, played as cards).
 assert.equal(nextGame({choice:{name:'counter-siege',title:'Counter Siege',mode:'cards',theme:''}}),'Fully Loaded · Counter Siege');
 assert.equal(nextGame({choice:{name:'counter-siege',title:'Counter Siege',mode:'cards',theme:'kitchen'},playing:{name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'}}),'Fully Loaded · Counter Siege · kitchen','the choice, in the lobby');
 assert.equal(nextGame({choice:{name:'counter-siege',title:'Counter Siege',mode:'cards',theme:'kitchen'},playing:{name:'counter-siege',title:'Counter Siege',mode:'cards',theme:'kitchen'}},true),'Fully Loaded · Counter Siege · kitchen','the map played, once the fridge has opened');
 // The other modes are as they were.
 assert.deepEqual(options({maps},'siege').map(o=>o.value),['random','counter-siege','counter-siege-v2','horde-night']);assert.deepEqual(options({maps},'forts').map(o=>o.value),['random','pillow-fort']);assert.equal(options({maps},'ffa')[0].value,'random');
 assert.deepEqual(MODES.slice(0,3).map(m=>MODE_NAME[m]),['Siege','Forts','Free For All']);
});

test('the mode and the map are chosen apart: a mode lists every map it can be played on, and greys the rest with the server\'s reason',()=>{
 // The roster as the server sends it (crates/server lobby.rs `Entry::listed`): each map's own mode, the modes it can be played in, and why not the others.
 const no=(title,mode)=>`${title} has no Forts layout (two bases, a field and a bacon spot): it cannot be played as ${MODE_NAME[mode]}.`;
 const maps=[
  {name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen',modes:['siege','ffa','cards'],why:{forts:no('Counter Siege','forts'),kungfu:'Counter Siege is not a Kung Fu map: it cannot be played as Kung Fu.'}},
  {name:'horde-night',title:'Horde Night',mode:'siege',theme:'night',modes:['siege','ffa'],why:{forts:no('Horde Night','forts'),cards:'Fully Loaded is played on Counter Siege.',kungfu:'Horde Night is not a Kung Fu map: it cannot be played as Kung Fu.'}},
  {name:'pillow-fort',title:'Countertop Fort',mode:'forts',theme:'kitchen',modes:['siege','forts','ffa'],why:{cards:'Fully Loaded is played on Counter Siege.',kungfu:'Countertop Fort is not a Kung Fu map: it cannot be played as Kung Fu.'}},
  {name:'kung-fu-dojo',title:'Kung Fu Dojo',mode:'kungfu',theme:'kitchen',modes:['kungfu'],why:{siege:'Kung Fu Dojo is a Kung Fu map: it cannot be played as Siege.',forts:'Kung Fu Dojo is a Kung Fu map: it cannot be played as Forts.',ffa:'Kung Fu Dojo is a Kung Fu map: it cannot be played as Free For All.',cards:'Fully Loaded is played on Counter Siege.'}},
 ];
 // A siege plays a Forts map too (it says what it was made for); the dojo is greyed, with the reason.
 assert.deepEqual(options({maps},'siege'),[{value:'random',text:'Random'},{value:'counter-siege',text:'Counter Siege · kitchen'},{value:'horde-night',text:'Horde Night · night'},{value:'pillow-fort',text:'Countertop Fort · Forts · kitchen'},
  {value:'kung-fu-dojo',text:'Kung Fu Dojo · Kung Fu · kitchen · is a Kung Fu map',disabled:true,why:'Kung Fu Dojo is a Kung Fu map: it cannot be played as Siege.'}]);
 // Forts: the maps with its layout first, then the rest greyed, each saying why.
 const forts=options({maps},'forts');
 assert.deepEqual(forts.filter(o=>!o.disabled).map(o=>o.value),['random','pillow-fort']);
 assert.deepEqual(forts.filter(o=>o.disabled).map(o=>[o.value,o.text]),[['counter-siege','Counter Siege · Siege · kitchen · has no Forts layout (two bases, a field and a bacon spot)'],['horde-night','Horde Night · Siege · night · has no Forts layout (two bases, a field and a bacon spot)'],['kung-fu-dojo','Kung Fu Dojo · Kung Fu · kitchen · is a Kung Fu map']]);
 assert.equal(forts.find(o=>o.value==='horde-night').why,no('Horde Night','forts'));
 // Free For All: every gun map, and the dojo greyed.
 assert.deepEqual(options({maps},'ffa').map(o=>[o.value,!!o.disabled]),[['random',false],['counter-siege',false],['horde-night',false],['pillow-fort',false],['kung-fu-dojo',true]]);
 // Fully Loaded and Kung Fu: their one map, nothing greyed.
 assert.deepEqual(options({maps},'cards').map(o=>o.value),['counter-siege']);assert.deepEqual(options({maps},'kungfu').map(o=>o.value),['random','kung-fu-dojo']);
 assert.ok(holds(maps[2],'siege')&&!holds(maps[1],'forts')&&holds(maps[3],'kungfu'));
 // What is next names the mode chosen, not the map's own.
 assert.equal(nextGame({choice:{name:'pillow-fort',title:'Countertop Fort',mode:'siege',theme:'kitchen'}}),'Siege · Countertop Fort · kitchen');
 // Free For All's rounds run out, as every gun mode's do: the line no longer says otherwise.
 assert.doesNotMatch(modeText({mode:'ffa'}),/unlimited/);
});

test('Fully Loaded: the address asks for it, and what the page sends for it is a mode (the server picks Counter Siege), never a map it would refuse',()=>{
 assert.deepEqual(asked('?mp&mode=cards'),{mode:'cards',map:null});assert.deepEqual(asked('?mp&mode=cards&map=counter-siege'),{mode:'cards',map:'counter-siege'});assert.equal(asked('?mp&sandbox&mode=cards'),null);
 const siege={name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'},cards={...siege,mode:'cards'},random={name:'random',title:'Random',mode:'siege',theme:'',random:true};
 assert.deepEqual(choosing({mode:'cards',map:null},siege),[{type:'choose',mode:'cards'}],'from the mode alone: the mode');
 assert.deepEqual(choosing({mode:'cards',map:null},random),[{type:'choose',mode:'cards'}]);
 assert.deepEqual(choosing({mode:'cards',map:'counter-siege'},siege),[{type:'choose',mode:'cards'},{type:'choose',map:'counter-siege'}],'Counter Siege is what the server would draw anyway');
 assert.deepEqual(choosing({mode:'cards',map:'counter-siege'},cards),[],'chosen already');
 assert.deepEqual(choosing({mode:'siege',map:null},cards),[{type:'choose',mode:'siege'}],'and back');
});

test('Kung Fu: a fifth mode, its maps those made for it (the dojo), with Random; the address can ask for it',()=>{
 const maps=[{name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'},{name:'pillow-fort',title:'Pillow Fort',mode:'forts',theme:'kitchen'},{name:'kung-fu-dojo',title:'Kung Fu Dojo',mode:'kungfu',theme:'kitchen'}];
 assert.equal(MODES[4],'kungfu');assert.equal(MODE_NAME.kungfu,'Kung Fu');assert.equal(KUNGFU_MAP,'kung-fu-dojo');
 assert.deepEqual(options({maps},'kungfu').map(o=>[o.value,o.text]),[['random','Random'],['kung-fu-dojo','Kung Fu Dojo · kitchen']]);
 assert.deepEqual(options({maps},'siege').map(o=>o.value),['random','counter-siege'],'the dojo is not a siege map');
 assert.ok(options({maps},'ffa').some(o=>o.text==='Pillow Fort · Forts · kitchen')&&!options({maps},'ffa').some(o=>o.value==='kung-fu-dojo'),'Free For All lists every gun map with what it was made for, and the dojo is Kung Fu\'s alone');
 assert.match(modeText({mode:'kungfu'}),/^Kung Fu · no guns, hands and feet · seven styles · first to ten KOs/);
 assert.equal(nextGame({choice:{name:'kung-fu-dojo',title:'Kung Fu Dojo',mode:'kungfu',theme:'kitchen'}}),'Kung Fu · Kung Fu Dojo · kitchen');
 assert.deepEqual(asked('?mp&mode=kungfu'),{mode:'kungfu',map:null});
 assert.deepEqual(choosing({mode:'kungfu',map:'kung-fu-dojo'},{name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'}),[{type:'choose',mode:'kungfu'},{type:'choose',map:'kung-fu-dojo'}]);
 // The assets folder has the map, made for the mode.
 const dojo=JSON.parse(readFileSync(new URL('../assets/maps/kung-fu-dojo.json',import.meta.url),'utf8'));assert.deepEqual([dojo.mode,dojo.name],['kungfu','Kung Fu Dojo']);
});

test('the address can ask the leader for a game (mode=, map=), but not in a sandbox; only what differs is chosen, the mode first',()=>{
 assert.deepEqual(asked('?mp&mode=ffa&map=pillow-fort'),{mode:'ffa',map:'pillow-fort'});
 assert.deepEqual(asked('?mp&map=horde-night'),{mode:null,map:'horde-night'});
 assert.equal(asked('?mp&sandbox&mode=ffa'),null,'a sandbox is set up by the game\'s join');
 assert.equal(asked('?mp'),null);assert.equal(asked('?mp&mode=tag&map=../x'),null,'nonsense is not asked for');
 const siege={name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen'},random={name:'random',title:'Random',mode:'siege',theme:'',random:true};
 assert.deepEqual(choosing({mode:'ffa',map:'pillow-fort'},siege),[{type:'choose',mode:'ffa'},{type:'choose',map:'pillow-fort'}]);
 assert.deepEqual(choosing({mode:'siege',map:'counter-siege'},siege),[],'chosen already');
 assert.deepEqual(choosing({mode:'ffa',map:'counter-siege'},siege),[{type:'choose',mode:'ffa'},{type:'choose',map:'counter-siege'}],'a new mode may draw another map: the map again');
 assert.deepEqual(choosing({mode:null,map:'random'},random),[]);assert.deepEqual(choosing({mode:null,map:'counter-siege'},random),[{type:'choose',map:'counter-siege'}]);
 assert.deepEqual(choosing({mode:'forts',map:null},siege),[{type:'choose',mode:'forts'}]);assert.deepEqual(choosing(null,siege),[]);
});

test('replays: the server\'s list as rows of text, each watched with ?replay= and saved from /replays/<id>, and only for an id the server could have made',()=>{
 const entry={id:'20261118-133456-counter-siege',date:'2026-11-18T13:34:56Z',mode:'forts',map:'Pillow Fort',players:['BUNNER','Nell'],duration:252.4,winner:'Ketchup Connoisseurs',complete:true};
 assert.deepEqual(replayText(entry),{title:'2026-11-18 13:34 · Pillow Fort · Forts · 4:12',detail:'Ketchup Connoisseurs won · BUNNER, Nell'});
 assert.equal(replayText({...entry,winner:null,complete:false}).detail,'BUNNER, Nell · cut short');
 assert.equal(replayLength(59.9),'0:59');assert.equal(replayLength(3600),'60:00');assert.equal(replayLength('x'),'0:00');
 assert.equal(replayHref(entry.id),`?replay=${entry.id}`);assert.equal(replayHref('../etc/passwd'),null);assert.equal(replayHref('a b'),null);
 const made=[],element=tag=>{const e={tag,className:'',children:[],append(...kids){e.children.push(...kids)},set innerHTML(_){throw new Error('markup must never be assigned')}};made.push(e);return e},doc={createElement:element};
 const evil='<img src=x onerror=alert(1)>',row=replayRow(doc,{...entry,map:evil,players:[evil]});
 const flat=e=>[e,...e.children.flatMap(flat)],texts=flat(row).map(e=>e.textContent).filter(Boolean);
 assert.ok(texts[0].includes(evil)&&texts[1].includes(evil),'names and maps as text content');
 const links=flat(row).filter(e=>e.tag==='a');assert.deepEqual(links.map(a=>a.href),[`?replay=${entry.id}`,`replays/${entry.id}`]);
 assert.equal(flat(replayRow(doc,{...entry,id:'"><script>'})).filter(e=>e.tag==='a').length,0,'no link for an id that is not one');
});

test('a replay that ended the page plays in light mode next time, and is not started again by itself if it ends it there too', ()=>{
 const now=1_800_000_000_000,id='20261120-112428-countertop-fort',note=(over)=>JSON.stringify({id,at:now-60e3,light:false,...over});
 assert.equal(REPLAY_GUARD,'hotdog-replay-open');
 assert.equal(replayStart(null,id,false,now),'normal','nothing kept: as it is');
 assert.equal(replayStart(note(),id,false,now),'light','the note left: the page was ended while it played');
 assert.equal(replayStart(note({light:true}),id,true,now),'stop','ended in light mode too');
 assert.equal(replayStart(note(),id,true,now),'light','light mode asked for, after an end in the full one');
 assert.equal(replayStart(null,id,true,now),'light','light mode asked for');
 assert.equal(replayStart(note({id:'another'}),id,false,now),'normal','another replay\'s note');
 assert.equal(replayStart(note({at:now-REPLAY_RECENT-1}),id,false,now),'normal','long ago');
 assert.equal(replayStart('{not json',id,false,now),'normal');assert.equal(replayStart('7',id,false,now),'normal');
});

test('light mode is WebGL 2 at the lowest graphics, the replay and the rest of the address kept; full quality takes it back', ()=>{
 const light=new URLSearchParams(lightAddress('?replay=abc&webgpu&scale=90&mp'));
 assert.deepEqual([light.get('replay'),light.has('webgl2'),light.has('webgpu'),light.get('quality'),light.has('light'),light.has('scale'),light.has('mp')],['abc',true,false,'lowest',true,false,true]);
 assert.ok(!lightAddress('?replay=abc').includes('light='),'bare words stay bare');
 assert.equal(fullAddress(lightAddress('?replay=abc')),'?replay=abc');
});

test('a replay that cannot be played is said so by its kind, with a retry only where one may help', ()=>{
 assert.deepEqual(replayTrouble('missing'),['Replay not found',false]);
 assert.equal(replayTrouble('network')[1],true);assert.equal(replayTrouble('version')[1],false);assert.equal(replayTrouble('memory')[1],true);
 assert.equal(replayTrouble('anything else')[0],'This replay cannot be played');
});

test('Titan: a sixth mode, its map the one made for it (the siege and Free For All may be played there too); the leader chooses who is the titan, and everyone sees who it is',()=>{
 assert.deepEqual([MODES.at(-1),MODE_NAME.titan,TITAN_MAP],['titan','Titan','titans-kitchen']);
 // The roster as the server sends it: Titan's Kitchen holds Titan, the siege, Free For All and the cards; no other map holds Titan.
 const maps=[
  {name:'counter-siege',title:'Counter Siege',mode:'siege',theme:'kitchen',modes:['siege','ffa','cards'],why:{titan:'Counter Siege has no Titan layout: it cannot be played as Titan.'}},
  {name:'kung-fu-dojo',title:'Kung Fu Dojo',mode:'kungfu',theme:'kitchen',modes:['kungfu'],why:{titan:'Kung Fu Dojo is a Kung Fu map: it cannot be played as Titan.'}},
  {name:'titans-kitchen',title:"Titan's Kitchen",mode:'titan',theme:'night',modes:['siege','ffa','cards','titan'],why:{forts:"Titan's Kitchen has no Forts layout"}},
 ];
 assert.deepEqual(options({maps},'titan').map(o=>[o.value,o.text,!!o.disabled]),[['random','Random',false],['titans-kitchen',"Titan's Kitchen · night",false]],'only its own map, nothing greyed');
 assert.ok(options({maps},'ffa').some(o=>o.value==='titans-kitchen'&&!o.disabled&&o.text==="Titan's Kitchen · Titan · night"),'Free For All plays on it, saying what it was made for');
 assert.ok(options({maps},'siege').some(o=>o.value==='titans-kitchen'&&!o.disabled));
 // A server that does not list the modes: the maps made for Titan.
 assert.deepEqual(options({maps:maps.map(({modes,why,...e})=>e)},'titan').map(o=>o.value),['random','titans-kitchen']);
 assert.match(modeText({mode:'titan'}),/^Titan · one giant hotdog against everyone else/);
 assert.equal(nextGame({choice:{name:'titans-kitchen',title:"Titan's Kitchen",mode:'titan',theme:'night'}}),"Titan · Titan's Kitchen · night");
 assert.deepEqual(asked('?mp&mode=titan'),{mode:'titan',map:null});
 // The chooser: auto, a bot, a random player, me, then every other player by name (the value sent is its member id).
 const members=[{id:1,name:'Ann',player:1},{id:2,name:'Ben',player:2},{id:3,name:'<b>Cy</b>',player:3}];
 assert.deepEqual(TITAN_PICKS,['auto','bot','random','me']);
 assert.deepEqual(titanOptions({members},1).map(o=>o.value),['auto','bot','random','me','2','3']);
 assert.equal(titanOptions({members},1).at(-1).text,'<b>Cy</b>','a name as text');
 const roster=titan=>({mode:'titan',members,titan});
 assert.deepEqual([titanValue(roster(null),1),titanValue(roster({pick:'bot'}),1),titanValue(roster({pick:'random'}),1),titanValue(roster({pick:'member',member:1}),1),titanValue(roster({pick:'member',member:3}),1),titanValue(roster({pick:'me',member:1}),2)],
  ['auto','bot','random','me','3','1']);
 // Who it is, before anyone readies up: by name once the server has settled it, "You" on that player's page, a bot, or how it will be drawn.
 assert.equal(titanText(roster({pick:'member',member:2,player:2,name:'Ben'}),1),'The titan: Ben');
 assert.equal(titanText(roster({pick:'member',member:2,player:2,name:'Ben'}),2),'You are the titan');
 assert.equal(titanText(roster({pick:'member',member:3,player:null,name:null}),1),'The titan: <b>Cy</b>');
 assert.equal(titanText(roster({pick:'bot',member:null,player:null,name:null}),1),'The titan: a bot');
 assert.match(titanText(roster({pick:'random',member:null,player:null,name:null}),1),/random player, drawn as the round starts/);
 assert.match(titanText(roster({pick:'auto',member:null,player:null,name:null}),1),/a bot under three players/);
 assert.equal(titanText({mode:'siege',members}),'');
 // The assets folder has the map, made for the mode.
 const arena=JSON.parse(readFileSync(new URL('../assets/maps/titans-kitchen.json',import.meta.url),'utf8'));assert.deepEqual([arena.mode,arena.name,arena.floor],['titan',"Titan's Kitchen",true]);
});

test('a page that was ended says what it was doing', () => {
 assert.equal(TRAIL,'hotdog-trail');
 const note=trailNote({now:5,up:11.4,what:'replay',readout:' 58 FPS  ·  WebGPU · Auto · Low · 45% ',canvas:'578x1250',screen:'428x926 at 3x',memory:412*2**20,replay:{state:'playing',clock:10400}});
 assert.deepEqual(note,{at:5,up:11,what:'replay',readout:'58 FPS · WebGPU · Auto · Low · 45%',canvas:'578x1250',screen:'428x926 at 3x',mb:412,state:'playing',clock:10});
 assert.equal(trailLine(JSON.stringify(note)),'ended 11 s after loading, at 0:10 of the replay, 58 FPS · WebGPU · Auto · Low · 45%, drawing 578x1250, screen 428x926 at 3x, memory 412 MB');
 // A replay whose file had not come, and the game itself: said so; what is not known is left out.
 assert.equal(trailLine(trailNote({now:1,up:3,what:'replay',replay:{state:'loading'}})),'ended 3 s after loading, the replay loading');
 assert.equal(trailLine(trailNote({now:1,up:75.2,what:'game',readout:'60 FPS',memory:NaN})),'ended 75 s after loading, in the game, 60 FPS');
 assert.equal(trailLine(trailNote({now:1,up:2,what:'replay',replay:{state:'playing',clock:125000}})),'ended 2 s after loading, at 2:05 of the replay');
 // Nothing kept, or something else under the name: nothing said.
 for(const none of [null,undefined,'','{','[]','"x"','{"up":"soon"}',7])assert.equal(trailLine(none),'');
});

test('alone with no map named, a mode that needs its own map gets it', () => {
 assert.deepEqual(['forts','titan','siege','ffa','cards','kungfu',null,undefined,'toString'].map(ownMap),['pillow-fort','titans-kitchen','','','','kung-fu-dojo','','','']);
 // The files are there, and are made for those modes.
 for(const mode of ['forts','kungfu','titan']){const map=JSON.parse(readFileSync(new URL(`../assets/maps/${ownMap(mode)}.json`,import.meta.url),'utf8'));assert.equal(map.mode,mode)}
});
