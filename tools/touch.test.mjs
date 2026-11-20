// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The touch controls' own sums (touch.js): how far the stick is pushed, and which buttons a state shows.
//   node --test tools/touch.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import {steer,shown,CAN,BUTTON,REACH,DEAD,OWN} from '../touch.js';

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
 assert.deepEqual(names(~0>>>0&~CAN.LOBBY),['FIRE','SCOPE']);
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
 for(const [name] of shown(~0>>>0&~CAN.LOBBY&~CAN.BACON&~CAN.CARRY&~CAN.TABLE))assert.ok(name in BUTTON,name);
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
