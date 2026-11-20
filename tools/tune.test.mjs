// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The sandbox panel's sums (tune.js): the schema grouped, values shown and sent to their step, what differs from the defaults.
//   node --test tools/tune.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import {sections,decimals,format,snap,valueOf,differs,changed,assign,GLOBAL,KEYS} from '../tune.js';

const field=(key,extra={})=>({key,group:key.split('.')[0],field:key.split('.')[1],label:key,unit:'',min:0,max:10,step:0.05,default:1.5,...extra});
const tune={sandbox:true,editable:true,online:false,
 groups:[{id:'melee',title:'Melee strike'},{id:'mustard',title:'Mustard pistol'},{id:'sniper',title:'Ketchup sniper'},{id:'stim',title:'Stim'},{id:'move',title:'Movement'},{id:'mayo',title:'Mayo squirt bottle'}],
 schema:[field('mustard.speed'),field('mustard.delay',{step:0.01,default:0.25}),field('sniper.speed',{max:30,step:0.5,default:9}),field('stim.time',{step:1,default:6}),field('move.walk',{default:3}),field('mustard.burst',{step:1,default:1}),field('relish.splash',{default:2})],
 values:{'mustard.speed':1.1,'mustard.delay':0.25,'sniper.speed':9,'stim.time':'8','move.walk':3},
 text:'mustard.speed=1.1 stim.time=8'};

test('the schema grouped as the game orders the groups, fields in schema order, empty groups left out', ()=>{
 const list=sections(tune);
 assert.deepEqual(list.map(s=>s.id),['mustard','sniper','stim','move','relish'],'melee and mayo have no fields; relish is not listed but has one');
 assert.deepEqual(list[0].fields.map(f=>f.key),['mustard.speed','mustard.delay','mustard.burst']);
 assert.deepEqual(list.filter(s=>s.global).map(s=>s.id),['stim','move']);assert.deepEqual(GLOBAL,['stim','move']);
 assert.equal(list.find(s=>s.id==='relish').title,'relish','a group the game did not name is named by its id');
 assert.deepEqual(sections(null),[]);assert.deepEqual(sections({schema:[null,{nokey:1}],groups:'x'}),[],'nonsense is ignored');
});

test('values to the step\'s decimals, without trailing zeros', ()=>{
 assert.equal(decimals(0.05),2);assert.equal(decimals(1),0);assert.equal(decimals(0.5),1);assert.equal(decimals(1e-7),7);assert.equal(decimals(undefined),3);
 assert.equal(format(1.1000000000000001,0.05),'1.1');assert.equal(format(9,0.5),'9');assert.equal(format(0.123456,undefined),'0.123');assert.equal(format(-0.001,0.01),'0');
 assert.equal(format('',0.1),'');assert.equal(format('abc',0.1),'');
});

test('a value typed or dragged is kept in range, and nonsense is the default', ()=>{
 const f=field('mustard.speed');
 assert.equal(snap(12,f),10);assert.equal(snap(-3,f),0);assert.equal(snap('1.234',f),1.23);assert.equal(snap('',f),1.5);assert.equal(snap('x',f),1.5);
 assert.equal(snap(7,{key:'a',default:1}),7,'no range: as it is');
});

test('which values differ from their defaults, as far as the panel shows them', ()=>{
 assert.deepEqual(changed(tune),['mustard.speed','stim.time']);
 assert.equal(valueOf(tune,tune.schema[3]),8,'a number sent as text');assert.equal(valueOf(tune,tune.schema[5]),1,'a key with no value: its default');
 assert.equal(differs(field('a.b'),1.5000001),false,'below the step\'s decimals is not a change');assert.equal(differs(field('a.b'),1.55),true);
 assert.deepEqual(changed({schema:tune.schema}),[],'no values: nothing changed');assert.deepEqual(changed(undefined),[]);
});

test('the commands the game drains', ()=>{
 const [speed,delay]=tune.schema;
 assert.equal(assign([[speed,1.1]]),'mustard.speed=1.1');
 assert.equal(assign([[speed,'1.10'],[delay,0.333333]]),'mustard.speed=1.1 mustard.delay=0.33','several, space-separated, to the step');
 assert.equal(assign([[speed,99]]),'mustard.speed=10','in range');
 assert.deepEqual(KEYS,['F2','Backquote']);
});
