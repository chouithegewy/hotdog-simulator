// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The sandbox link composer's sums (devlink.js, used by dev.html): the address of a game from the page's choices.
//   node --test tools/devlink.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import {compose,query,weaponList,WEAPONS,QUICK,PORTS,MODES,CARDS_MAP} from '../devlink.js';

test('online or offline first, then sandbox, then the rest in order; switches are bare keys', ()=>{
 assert.equal(compose({host:'192.0.2.10',port:8141,weapons:['mustard','sniper'],ammo:true,bots:2,botmode:'idle'}),'http://192.0.2.10:8141/?mp&sandbox&weapons=mustard,sniper&ammo=inf&bots=2&botmode=idle');
 assert.equal(compose({host:'h',port:8137}),'http://h:8137/?mp&sandbox');
 assert.equal(compose({host:'h',port:8137,online:false,sandbox:false}),'http://h:8137/?play');
 assert.equal(query({mode:'forts',map:'pillow-fort.json',weapons:'all',ammo:true,god:true,bots:0,botmode:'walk',zombies:true,field:true,stim:true,respawn:'here',check:true,webgl2:true}),
  '?mp&sandbox&mode=forts&map=pillow-fort&weapons=all&ammo=inf&god&bots=0&botmode=walk&zombies=on&field=on&stim=inf&respawn=here&check&webgl2');
});

test('defaults and unknowns are left out', ()=>{
 assert.equal(query({botmode:'fight',respawn:'start',bots:null,mode:'',map:''}),'?mp&sandbox','the defaults say nothing');
 assert.equal(query({bots:''}),'?mp&sandbox','bots: default');
 assert.equal(query({bots:7}),'?mp&sandbox','more bots than there are places');
 assert.equal(query({bots:'3'}),'?mp&sandbox&bots=3','a number from a select');
 assert.equal(query({mode:'tag',map:'../etc/passwd',botmode:'dance',respawn:'moon'}),'?mp&sandbox');
 assert.equal(query({weapons:['mustard','bazooka']}),'?mp&sandbox&weapons=mustard','an unknown gun');
 assert.equal(query({weapons:[]}),'?mp&sandbox','no guns: the game\'s own');
});

test('a game that is not a sandbox keeps only its mode, map and extras', ()=>{
 assert.equal(query({sandbox:false,mode:'ffa',map:'counter-siege',weapons:'all',ammo:true,god:true,bots:2,zombies:true,check:true}),'?mp&mode=ffa&map=counter-siege&check');
});

test('Fully Loaded is a mode of its own: mode=cards, on Counter Siege', ()=>{
 assert.deepEqual(MODES.map(([m,name])=>`${m}:${name}`),['siege:Siege','forts:Forts','ffa:Free For All','cards:Fully Loaded'],'after Free For All');
 assert.equal(CARDS_MAP,'counter-siege');
 assert.equal(compose({host:'h',port:8141,mode:'cards'}),'http://h:8141/?mp&sandbox&mode=cards');
 assert.equal(compose({host:'h',port:8141,sandbox:false,mode:'cards'}),'http://h:8141/?mp&mode=cards','the lobby\'s leader chooses it');
 assert.equal(query({sandbox:false,mode:'cards',map:'counter-siege.json',weapons:'all',ammo:true,god:true,bots:2,check:true}),'?mp&mode=cards&map=counter-siege&check','a game that is not a sandbox keeps only its mode, map and extras');
 assert.equal(query({online:false,sandbox:false,mode:'cards'}),'?play&mode=cards');
 // The other modes are still said, and what is not one still is not.
 assert.equal(query({mode:'siege'}),'?mp&sandbox&mode=siege');assert.equal(query({mode:'forts'}),'?mp&sandbox&mode=forts');assert.equal(query({mode:'ffa'}),'?mp&sandbox&mode=ffa');assert.equal(query({mode:'card'}),'?mp&sandbox');
});

test('guns: in the game\'s order, or all', ()=>{
 assert.equal(weaponList(['mayo','mustard','sniper']),'mustard,sniper,mayo');
 assert.equal(weaponList('all'),'all');assert.equal(weaponList(['mustard','all']),'all');assert.equal(weaponList('relish,syrup'),'relish,syrup');
 assert.deepEqual(WEAPONS.map(w=>w.id),[1,2,3,4,5,6,7],'numbered as the game numbers them');
});

test('the host as typed or pasted', ()=>{
 assert.equal(compose({host:' http://example.lan:9000/?mp ',port:8142}),'http://example.lan:8142/?mp&sandbox','a pasted address keeps only its host');
 assert.equal(compose({host:'::1',port:8137}),'http://[::1]:8137/?mp&sandbox');
 assert.equal(compose({host:'',port:8137}),'http://localhost:8137/?mp&sandbox');
 assert.equal(compose({host:'h',port:'',protocol:'https:'}),'https://h/?mp&sandbox','no port: the scheme\'s own');
});

test('the quick links', ()=>{
 const links=QUICK.map(q=>compose({host:'h',port:8141,...q.options}));
 assert.deepEqual(links,['http://h:8141/?mp&sandbox&weapons=mustard,sniper','http://h:8141/?mp&sandbox&weapons=all&ammo=inf&god&bots=4&botmode=idle','http://h:8141/?mp&sandbox&mode=ffa','http://h:8141/?mp&mode=ffa']);
 assert.deepEqual(PORTS.map(([p])=>p),[8137,8141,8142]);
});
