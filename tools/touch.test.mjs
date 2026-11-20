// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The touch controls' own sums (touch.js): how far the stick is pushed, which buttons a state shows, and a replay's bar
// and gestures.
//   node --test tools/touch.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import {steer,shown,CAN,BUTTON,REACH,DEAD,OWN,clock,replayWords,along,pair,tapped} from '../touch.js';

const names=state=>shown(state).map(([name])=>name);
const label=(state,name)=>shown(state).find(([n])=>n===name)?.[1];

test('the stick walks as far as it is pushed, past a dead zone, and no further than the pace', ()=>{
 assert.deepEqual(steer(0,0),[0,0]);
 assert.deepEqual(steer(0,REACH*DEAD*0.9),[0,0],'a resting thumb does not walk');
 const [x,y]=steer(0,-REACH);assert.ok(Math.abs(x)<1e-9&&Math.abs(y+1)<1e-9,'a full push up is forwards at the pace');
 assert.ok(Math.abs(Math.hypot(...steer(REACH*3,REACH*3))-1)<1e-9,'beyond the reach is still the pace');
 const half=Math.hypot(...steer(REACH*(DEAD+(1-DEAD)/2),0));assert.ok(Math.abs(half-0.5)<1e-9,'half way past the dead zone is half the pace');
 const [sx,sy]=steer(REACH*0.6,REACH*0.8);assert.ok(Math.abs(sy/sx-0.8/0.6)<1e-9,'the way it is pushed is kept');
});

test('nothing outside a round of its own; the lobby, a finished round and a watching player get their own', ()=>{
 assert.deepEqual(names(0),[]);
 assert.deepEqual(names(CAN.LOBBY),[]);
 assert.deepEqual(names(CAN.OVER),['ROUND']);
 assert.deepEqual(names(CAN.PLAY),[],'dead, nobody to watch');
 // Dead with a hotdog to watch: Fire is Next (a drag turns the death camera, a pinch zooms it).
 assert.deepEqual(names(CAN.PLAY|CAN.WATCH),['FIRE']);assert.equal(label(CAN.PLAY|CAN.WATCH,'FIRE'),'Next');
});

test('a living player: the buttons follow what it can do', ()=>{
 const alive=CAN.PLAY|CAN.ALIVE;
 assert.deepEqual(names(alive|CAN.SECONDARY),['FIRE','JUMP','MELEE','CROUCH','PRONE','STIM','SECONDARY']);
 assert.ok(names(alive|CAN.PRIMARY|CAN.SECONDARY).includes('SWAP'),'two guns: swap');
 assert.ok(names(alive|CAN.SNIPER).includes('SCOPE'));
 assert.equal(label(alive|CAN.PRONE,'JUMP'),'Stand');
 assert.equal(label(alive|CAN.FAMINE,'MELEE'),'Bite');
 assert.equal(label(alive|CAN.FAMINE|CAN.EAT,'USE'),'Eat');
 assert.equal(label(alive|CAN.PICKUP,'USE'),'Pick up');
 assert.equal(label(alive|CAN.SWITCH,'USE'),'Switch','at a griddle\'s dial E switches it');assert.equal(label(alive|CAN.SWITCH|CAN.PICKUP,'USE'),'Pick up');assert.equal(label(alive|CAN.FAMINE|CAN.EAT|CAN.SWITCH,'USE'),'Eat');
 assert.ok(!names(alive).includes('USE'),'nothing to use, no button');
 assert.equal(label(alive|CAN.GRASP,'CARRY'),'Carry');
 const build=alive|CAN.BUILD|CAN.PRIMARY|CAN.SECONDARY;
 assert.equal(label(build,'FIRE'),'Locked');assert.equal(label(build,'READY'),'Ready');assert.equal(label(build|CAN.READY,'READY'),'Ready ✓');
 const carrying=names(build|CAN.CARRY);
 assert.equal(label(build|CAN.CARRY,'FIRE'),'Throw');assert.equal(label(build|CAN.CARRY,'CARRY'),'Put down');
 for(const gone of ['MELEE','SWAP','PRIMARY','SECONDARY','SCOPE'])assert.ok(!carrying.includes(gone),gone+' while carrying');
 const bacon=names(alive|CAN.BACON|CAN.PRIMARY|CAN.SECONDARY);
 for(const gone of ['FIRE','MELEE','PRONE','SWAP'])assert.ok(!bacon.includes(gone),gone+' with the bacon');
 assert.ok(bacon.includes('JUMP')&&bacon.includes('STIM')&&bacon.includes('CROUCH'),'the bacon carrier runs, jumps, crouches and stims');
});

test('at the table only flip and stop show', ()=>{
 const table=CAN.TABLE,kinds=state=>shown(state).map(([,,kind])=>kind);
 // Flip is the trigger and Stop the scope (src/touch.rs presses them as the left and right buttons ask of the table).
 for(const state of [table,table|CAN.GO,table|CAN.PICK])assert.deepEqual(names(state),['FIRE','SCOPE'],state);
 assert.equal(label(table|CAN.GO,'FIRE'),'Flip');assert.equal(label(table|CAN.GO,'SCOPE'),'Stop');
 assert.deepEqual(kinds(table|CAN.GO),['','']);
 // Another's go: both there, greyed. Picking who gets an action card: the trigger picks the hotdog looked at.
 assert.deepEqual(kinds(table),['dim','dim']);assert.equal(label(table,'FIRE'),'Flip');
 assert.equal(label(table|CAN.PICK,'FIRE'),'Pick');
 // Whatever else the state says, the round's buttons stay away.
 assert.deepEqual(names(~0>>>0&~CAN.LOBBY&~CAN.REPLAY),['FIRE','SCOPE']);
 assert.deepEqual([CAN.TABLE,CAN.GO,CAN.PICK],[1<<22,1<<23,1<<24],'as src/touch.rs numbers them');
});

test('picking who gets an action card, Stop becomes Me: the one way by touch to pick yourself', ()=>{
 const table=CAN.TABLE,kinds=state=>shown(state).map(([,,kind])=>kind);
 // The trigger picks the hotdog looked at, which is never your own; the scope (src/touch.rs: the right button) picks you.
 assert.equal(label(table|CAN.PICK,'SCOPE'),'Me');assert.equal(label(table|CAN.PICK,'FIRE'),'Pick');
 assert.deepEqual(kinds(table|CAN.PICK),['',''],'both are live');
 assert.equal(label(table|CAN.GO,'SCOPE'),'Stop','at its go it is still Stop');
 assert.equal(label(table,'SCOPE'),'Stop');assert.deepEqual(kinds(table),['dim','dim'],'another\'s go or pick: both greyed');
 assert.deepEqual(names(table|CAN.PICK),['FIRE','SCOPE'],'no new button: the same two');
});

test('every button the page shows is one the game reads', ()=>{
 for(const [name] of shown(~0>>>0&~CAN.LOBBY&~CAN.REPLAY&~CAN.BACON&~CAN.CARRY&~CAN.TABLE))assert.ok(name in BUTTON,name);
 assert.equal(Object.values(BUTTON).reduce((a,b)=>a|b,0),(1<<15)-1,'fifteen bits, as src/touch.rs has them');
});

test('in a round a small Hints button hides the hints and shows them again, as H does', ()=>{
 const alive=CAN.PLAY|CAN.ALIVE;
 assert.ok(!names(alive).includes('HINTS'),'only when the game says there are hints to show');
 assert.equal(label(alive|CAN.HINTS|CAN.HINTS_ON,'HINTS'),'Hide hints');
 assert.equal(label(alive|CAN.HINTS,'HINTS'),'Hints');
 assert.deepEqual(shown(alive|CAN.HINTS).find(([name])=>name==='HINTS')[2],'wide dim','hidden: greyed until tapped');
 assert.ok(!names(CAN.PLAY|CAN.HINTS).includes('HINTS'),'not dead');
 assert.ok(!names(CAN.LOBBY|CAN.HINTS).includes('HINTS'),'not in the lobby');
 assert.deepEqual([BUTTON.HINTS,CAN.HINTS,CAN.HINTS_ON,CAN.SWITCH],[1<<14,1<<25,1<<26,1<<27],'as src/touch.rs numbers them');
});

test('Kung Fu: the buttons are the fight\'s, the special lit when the chi is full, and nothing of the guns, the stim or carrying', ()=>{
 const fight=CAN.PLAY|CAN.ALIVE|CAN.FIGHT;
 assert.deepEqual([CAN.FIGHT,CAN.CHI],[1<<28,1<<29],'as src/touch.rs numbers them');
 assert.deepEqual(shown(fight).map(([name,label])=>`${name}:${label}`),['FIRE:Punch','SCOPE:Heavy','MELEE:Kick','JUMP:Jump','CROUCH:Crouch','CARRY:Block','USE:Grab','STIM:Dodge','PRONE:Special','SWAP:Style']);
 assert.equal(shown(fight).find(([name])=>name==='PRONE')[2],'dim','the special waits for full chi');
 assert.equal(shown(fight|CAN.CHI).find(([name])=>name==='PRONE')[2],'on');
 assert.equal(shown(fight|CAN.CROUCH).find(([name])=>name==='CROUCH')[2],'on');
 assert.ok(names(fight|CAN.HINTS).includes('HINTS'));
 assert.ok(!names(CAN.PLAY|CAN.FIGHT).includes('MELEE'),'dead, only Next');
});

test('touches on the page\'s own panels are left to them', ()=>{
 const own=OWN.split(',');
 for(const part of ['button','input','.lobby','.touch-menu','.chat-form','.tune'])assert.ok(own.includes(part),part);
});

test('a replay shows none of the game\'s buttons: its own bar, and touches on it are left to it', ()=>{
 assert.equal(CAN.REPLAY,2**30,'as src/touch.rs numbers it');
 assert.deepEqual(names(CAN.REPLAY),[]);
 assert.deepEqual(names(CAN.REPLAY|CAN.PLAY|CAN.ALIVE),[],'whatever else is set');
 assert.ok(OWN.split(',').includes('.touch-replay')&&OWN.split(',').includes('.replay-note'));
});

test('the replay\'s bar says the time, play or pause, and the view, from what the game tells', ()=>{
 assert.equal(clock(0),'0:00');assert.equal(clock(12_400),'0:12');assert.equal(clock(296_933),'4:56');assert.equal(clock(3_725_000),'1:02:05');assert.equal(clock(-5),'0:00');assert.equal(clock(undefined),'0:00');
 const st={state:'playing',clock:12_000,duration:296_933,paused:false,ended:false,mode:'first'};
 assert.deepEqual(replayWords(st),{time:'0:12 / 4:56',play:'Pause',view:'First person',share:12_000/296_933});
 assert.equal(replayWords({...st,paused:true}).play,'Play');
 assert.equal(replayWords({...st,ended:true,paused:true}).play,'Again');
 assert.equal(replayWords({...st,mode:'follow'}).view,'Follow');assert.equal(replayWords({...st,mode:'fly'}).view,'Free camera');
 assert.equal(replayWords({...st,duration:0}).share,0,'no length yet');
 assert.equal(replayWords({...st,clock:999_999}).share,1);
});

test('a tap or a drag on the timeline seeks to that share of it, and no further than its ends', ()=>{
 assert.equal(along(110,10,400),0.25);assert.equal(along(0,10,400),0);assert.equal(along(900,10,400),1);assert.equal(along(50,10,0),0);
});

test('two fingers fly as a stick from where they landed and pinch by how much further apart they are', ()=>{
 const start=[200,300];
 const still=pair(start,100,[150,300],[250,300]);assert.deepEqual([still.x,still.y,still.pinch,still.d],[0,0,0,100],'where they landed: nothing');
 const up=pair(start,100,[150,300-REACH],[250,300-REACH]);assert.ok(Math.abs(up.y+1)<1e-9&&Math.abs(up.x)<1e-9,'both moved up a reach: forwards at full pace');
 const apart=pair(start,100,[100,300],[300,300]);assert.ok(Math.abs(apart.pinch-Math.log(2))<1e-9&&apart.d===200,'twice as far apart');
 const together=pair(start,200,[175,300],[225,300]);assert.ok(together.pinch<0,'pinched together');
 assert.equal(pair(start,0,[200,300],[200,300]).pinch,0,'no distance to compare');
});

test('a tap on the picture is short and stays where it landed; anything else is a look', ()=>{
 assert.ok(tapped(120,2,3));assert.ok(!tapped(600,0,0),'held');assert.ok(!tapped(120,30,0),'dragged');
});

test('Titan: as the giant the buttons are its moves, the charge dimmed while it cools down, and nothing of the guns, the stim or carrying', ()=>{
 const giant=CAN.PLAY|CAN.ALIVE|CAN.GIANT;
 assert.equal(CAN.GIANT,2**31,'as src/touch.rs numbers it (1 << 31)');
 assert.deepEqual(shown(giant).map(([name,label])=>`${name}:${label}`),['FIRE:Swat','SCOPE:Smash','MELEE:Stomp','JUMP:Jump','CROUCH:Crouch','STIM:Charge']);
 assert.equal(shown(giant|CAN.STIM_WAIT).find(([name])=>name==='STIM')[2],'dim');
 assert.equal(shown(giant|CAN.CROUCH).find(([name])=>name==='CROUCH')[2],'on');
 // The bit survives the page's 32-bit arithmetic, and a small hotdog keeps the gun basics.
 assert.ok(shown((giant|0)>>>0).some(([name])=>name==='MELEE'));
 assert.ok(shown(CAN.PLAY|CAN.ALIVE|CAN.SNIPER).some(([,label])=>label==='Fire'));
 assert.ok(!shown(CAN.PLAY|CAN.GIANT).some(([,label])=>label==='Swat'),'dead, none of them');
});
