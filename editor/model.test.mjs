// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Editor document model: editing operations, history, snapping and picking. Run with `node --test editor/model.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {Doc,rectFromDrag,resizeRect,riseFromDrag,snapPoint,edgeSnap,itemBox,alignItems,fortified,holding,FOOTPRINT,FIXED,KIT_NAME,sinkAt} from './model.js';
import {normalize,stringify,analyze,compile,AMMO_BOX,RESPAWN,KITS,SINK_DEPTH,LOOSE} from '../tools/network/map.mjs';
import {PRESETS} from '../tools/network/presets.mjs';

const text=readFileSync(new URL('../assets/maps/counter-siege.json',import.meta.url),'utf8');
const open=()=>{const doc=new Doc();doc.load(text);return doc};
const idOf=(doc,kind,index)=>doc.map[kind][index].id;
const rectOf=(doc,id)=>doc.find(id).item.rect;
const normalizeIds=(doc,kind)=>doc.map[kind].map(item=>item.id);

test('a loaded map saves back byte for byte',()=>{
  assert.equal(open().toJSON(),text);
});

test('every object gets its own id and can be found by it',()=>{
  const doc=open(),ids=[...doc.items()].map(entry=>entry.item.id);
  // (Counter Siege's ten pickups: eight guns and the two boxes of ammunition every siege map has.)
  assert.equal(ids.length,5+3+10+4+1+2+2);
  assert.equal(new Set(ids).size,ids.length);
  const found=doc.find(idOf(doc,'blocks',2));
  assert.deepEqual([found.kind,found.index,found.item.rect],['blocks',2,[-.45,2.7,-.23,2.92]]);
  assert.equal(doc.find(-1),null);
});

test('loading rejects a broken file and keeps the open map',()=>{
  const doc=open();
  assert.throws(()=>doc.load('{"format":"other"}'),/format/);
  assert.equal(doc.toJSON(),text);
});

test('moving shifts rectangles and points on the 0.1 mm lattice',()=>{
  const doc=open(),block=idOf(doc,'blocks',2),pickup=idOf(doc,'pickups',0);
  assert.equal(doc.move([block,pickup],.1,-.05).ok,true);
  assert.deepEqual(rectOf(doc,block),[-.35,2.65,-.13,2.87]);
  assert.deepEqual(doc.find(pickup).item.p,[-.6,2.4]);
  doc.move([block],.00004,0);
  assert.deepEqual(rectOf(doc,block),[-.35,2.65,-.13,2.87]);
});

test('undo and redo step through edits and restore the selection',()=>{
  const doc=open(),block=idOf(doc,'blocks',2);
  doc.select([block]);
  doc.move([block],.1,0);
  doc.patch(block,{height:.4});
  assert.equal(doc.find(block).item.height,.4);
  assert.deepEqual([doc.canUndo,doc.canRedo],[true,false]);
  doc.undo();
  assert.equal(doc.find(block).item.height,.25);
  assert.deepEqual(rectOf(doc,block),[-.35,2.7,-.13,2.92]);
  doc.undo();
  assert.deepEqual(rectOf(doc,block),[-.45,2.7,-.23,2.92]);
  assert.deepEqual(doc.selection,[block]);
  assert.equal(doc.canUndo,false);
  doc.redo();doc.redo();
  assert.equal(doc.find(block).item.height,.4);
  assert.equal(doc.toJSON().includes('"height": 0.4'),true);
});

test('a new edit after undo discards the redo branch',()=>{
  const doc=open(),block=idOf(doc,'blocks',2);
  doc.move([block],.1,0);doc.undo();doc.move([block],0,.1);
  assert.equal(doc.canRedo,false);
  assert.deepEqual(rectOf(doc,block),[-.45,2.8,-.23,3.02]);
});

test('an edit that would break the map is refused with a reason and changes nothing',()=>{
  const doc=open(),before=doc.toJSON();
  const result=doc.patch(idOf(doc,'ramps',0),{base:.5});
  assert.equal(result.ok,false);
  assert.match(result.error,/rise/);
  assert.equal(doc.toJSON(),before);
  assert.equal(doc.canUndo,false);
});

test('the last start and the last zombie spawn cannot be removed',()=>{
  const doc=open();
  const result=doc.remove(doc.map.spawns.map(spawn=>spawn.id));
  assert.equal(result.ok,false);
  assert.match(result.error,/spawn/);
  assert.equal(doc.map.spawns.length,3);
  assert.equal(doc.remove([idOf(doc,'spawns',0),idOf(doc,'spawns',1)]).ok,true);
  assert.equal(doc.map.spawns.length,1);
});

test('removing objects also drops them from the selection',()=>{
  const doc=open(),a=idOf(doc,'blocks',2),b=idOf(doc,'blocks',3);
  doc.select([a,b]);
  doc.remove([a]);
  assert.deepEqual(doc.selection,[b]);
  assert.equal(doc.map.blocks.length,3);
  doc.undo();
  assert.deepEqual(doc.selection,[a,b]);
});

test('adding an object selects it and fills in the defaults',()=>{
  const doc=open();
  const {ok,id}=doc.add('blocks',{rect:[0,3,.2,3.2]});
  assert.equal(ok,true);
  assert.deepEqual(doc.find(id).item,{id,rect:[0,3,.2,3.2],height:.25,style:'steel'});
  assert.deepEqual(doc.selection,[id]);
  assert.equal(doc.add('pickups',{p:[0,3],weapon:9,ammo:1}).ok,false);
});

test('duplicating offsets the copies, keeps the originals and selects the copies',()=>{
  const doc=open(),block=idOf(doc,'blocks',3),start=idOf(doc,'starts',0);
  const {ok,ids}=doc.duplicate([block,start],.04,.04);
  assert.equal(ok,true);
  assert.equal(doc.map.blocks.length,5);
  assert.equal(doc.map.starts.length,6);
  assert.deepEqual(doc.find(ids[0]).item.rect,[.38,2.49,.62,2.68]);
  assert.deepEqual(rectOf(doc,block),[.34,2.45,.58,2.64]);
  assert.deepEqual(doc.selection,ids);
});

test('copied objects paste centred on a point, any number of times',()=>{
  const doc=open(),clip=doc.copy([idOf(doc,'blocks',2),idOf(doc,'pickups',0)]);
  const {ids}=doc.paste(clip,[1,3]);
  // The pair spans x -0.7..-0.23 and z 2.45..2.92; its centre moves to 1, 3.
  assert.deepEqual(doc.find(ids[0]).item.rect,[1.015,3.015,1.235,3.235]);
  assert.deepEqual(doc.find(ids[1]).item.p,[.765,2.765]);
  doc.paste(clip,[0,2.6]);
  assert.equal(doc.map.blocks.length,6);
});

test('a drag is one undo step however many frames it took, and Escape restores the start',()=>{
  const doc=open(),block=idOf(doc,'blocks',2);
  doc.begin();
  for(let i=1;i<=30;i++)doc.preview(map=>doc.moved(map,[block],i*.01,0));
  assert.deepEqual(rectOf(doc,block),[-.15,2.7,.07,2.92]);
  assert.equal(doc.commit().ok,true);
  doc.undo();
  assert.deepEqual(rectOf(doc,block),[-.45,2.7,-.23,2.92]);
  assert.equal(doc.canUndo,false);
  doc.begin();doc.preview(map=>doc.moved(map,[block],1,1));doc.cancel();
  assert.deepEqual(rectOf(doc,block),[-.45,2.7,-.23,2.92]);
  assert.equal(doc.canUndo,false);
});

test('a drag that ends where it began leaves no undo step',()=>{
  const doc=open(),block=idOf(doc,'blocks',2);
  doc.begin();doc.preview(map=>doc.moved(map,[block],.1,0));doc.preview(map=>doc.moved(map,[block],0,0));doc.commit();
  assert.equal(doc.canUndo,false);
});

test('a drag that ends in an invalid state snaps back and says why',()=>{
  const doc=open(),ramp=idOf(doc,'ramps',0);
  doc.begin();doc.preview(map=>doc.patched(map,ramp,{height:0}));
  const result=doc.commit();
  assert.equal(result.ok,false);
  assert.equal(doc.find(ramp).item.height,.28);
});

test('history shares untouched objects between steps instead of copying the map',()=>{
  const doc=open(),untouched=doc.map.blocks[0],pickups=doc.map.pickups;
  doc.move([idOf(doc,'blocks',2)],.1,0);
  assert.equal(doc.map.blocks[0],untouched);
  assert.equal(doc.map.pickups,pickups);
});

test('map settings are edited like everything else',()=>{
  const doc=open();
  assert.equal(doc.setSettings({name:'  Back counter  ',bounds:[-2,2.27,2,3.63]}).ok,true);
  assert.equal(doc.map.name,'Back counter');
  assert.equal(doc.level.cols,100);
  assert.equal(doc.setSettings({bounds:[0,0,.1,.1]}).ok,false);
  assert.equal(doc.setSettings({nav:{cell:.001,clearance:.035}}).ok,false);
  doc.undo();
  assert.equal(doc.map.name,'Counter Siege');
});

test('the compiled level and its analysis follow the map and are reused while it is unchanged',()=>{
  const doc=open(),level=doc.level,analysis=doc.analysis;
  assert.equal(doc.level,level);
  assert.deepEqual(analysis.issues,[]);
  doc.select([idOf(doc,'blocks',0)]);
  assert.equal(doc.analysis,analysis,'selecting does not recompute');
  doc.move([idOf(doc,'starts',2)],-.29,.29);
  assert.notEqual(doc.level,level);
  assert.deepEqual(doc.analysis.issues.map(issue=>issue.code),['start-blocked']);
});

test('listeners hear about map and selection changes',()=>{
  const doc=open(),heard=[];
  const stop=doc.on(event=>heard.push(event.type));
  doc.select([idOf(doc,'blocks',0)]);
  doc.move([idOf(doc,'blocks',0)],.01,0);
  doc.undo();
  stop();
  doc.redo();
  assert.deepEqual(heard,['selection','map','map']);
});

test('selection can be replaced, extended and toggled',()=>{
  const doc=open(),[a,b,c]=doc.map.blocks.map(block=>block.id);
  doc.select([a]);doc.select([b],{add:true});
  assert.deepEqual(doc.selection,[a,b]);
  doc.select([a,c],{toggle:true});
  assert.deepEqual(doc.selection,[b,c]);
  doc.select([]);
  assert.deepEqual(doc.selection,[]);
  doc.selectAll();
  assert.equal(doc.selection.length,27);
});

test('picking returns the topmost object: markers above blocks, blocks above decks',()=>{
  const doc=open();
  doc.move([idOf(doc,'pickups',0)],.36,.36);
  assert.equal(doc.pick(-.34,2.81,.005),idOf(doc,'pickups',0));
  assert.equal(doc.pick(-.4,2.75,.005),idOf(doc,'blocks',2));
  assert.equal(doc.pick(1.5,3.3,.005),idOf(doc,'blocks',1));
  assert.equal(doc.pick(1.7,3.55,.005),idOf(doc,'decks',0));
  assert.equal(doc.pick(1.6,2.8,.005),idOf(doc,'ramps',1));
  assert.equal(doc.pick(0,2.9,.005),null);
});

test('picking is forgiving near small markers and thin edges',()=>{
  const doc=open();
  assert.equal(doc.pick(.86,3.25,.02),idOf(doc,'starts',0));
  assert.equal(doc.pick(-.455,2.8,.01),idOf(doc,'blocks',2));
  assert.equal(doc.pick(-.47,2.8,.01),null);
});

test('a marquee picks everything it touches',()=>{
  const doc=open(),kinds=doc.pickAll([-.5,2.3,.1,2.75]).map(id=>doc.find(id).kind).sort();
  assert.deepEqual(kinds,['blocks','pickups','starts']);
});

test('a drag in any direction makes a well-formed rectangle no smaller than the minimum',()=>{
  assert.deepEqual(rectFromDrag([1,3],[.5,2.5]),[.5,2.5,1,3]);
  assert.deepEqual(rectFromDrag([1,3],[1.002,3.3]),[1,3,1.01,3.3]);
  assert.deepEqual(rectFromDrag([1,3],[.998,2.999]),[.99,2.99,1,3]);
});

test('resize handles move only their own edges and stop at the minimum size',()=>{
  const rect=[0,0,1,1];
  assert.deepEqual(resizeRect(rect,{x:1,z:0},1.5,9),[0,0,1.5,1]);
  assert.deepEqual(resizeRect(rect,{x:-1,z:-1},.25,.5),[.25,.5,1,1]);
  assert.deepEqual(resizeRect(rect,{x:-1,z:0},2,0),[.99,0,1,1]);
  assert.deepEqual(resizeRect(rect,{x:0,z:1},0,-3),[0,0,1,.01]);
});

test('a ramp rises towards where the drag ended, along its longer side',()=>{
  assert.equal(riseFromDrag([0,0],[.7,.2]),'+x');
  assert.equal(riseFromDrag([0,0],[-.7,.2]),'-x');
  assert.equal(riseFromDrag([0,0],[.1,.5]),'+z');
  assert.equal(riseFromDrag([0,0],[.1,-.5]),'-z');
});

test('points snap to the grid step, or stay put when snapping is off',()=>{
  assert.deepEqual(snapPoint([.123,3.456],.01),[.12,3.46]);
  assert.deepEqual(snapPoint([.123,3.456],.04),[.12,3.44]);
  assert.deepEqual(snapPoint([-.126,3.456],.01),[-.13,3.46]);
  assert.deepEqual(snapPoint([.12349,3.456],0),[.1235,3.456]);
});

test('a dragged edge snaps to a nearby edge of another object and reports the guide',()=>{
  const doc=open(),candidates=doc.snapEdges([idOf(doc,'ramps',0)]);
  // The deck's left edge is at x = 1.25: a ramp edge released within reach lands exactly on it.
  assert.deepEqual(edgeSnap(1.262,candidates.x,.02),{value:1.25,guide:1.25});
  assert.deepEqual(edgeSnap(1,candidates.x,.02),{value:1,guide:null});
  assert.deepEqual(edgeSnap(3.6,candidates.z,.02),{value:3.61,guide:3.61});
  // The object being dragged must not snap to itself.
  assert.equal(candidates.x.includes(.55),false);
  // The arena edge is a candidate too.
  assert.equal(candidates.x.includes(-1.8),true);
});

test('objects report the box they occupy on the plan',()=>{
  const doc=open();
  assert.deepEqual(itemBox(doc.map.blocks[2]),[-.45,2.7,-.23,2.92]);
  assert.deepEqual(itemBox(doc.map.starts[0],.03).map(v=>+v.toFixed(6)),[.8,3.2,.86,3.26]);
});

test('aligning moves objects flush to the selection\'s edge in one undo step',()=>{
  const doc=open(),a=idOf(doc,'blocks',2),b=idOf(doc,'blocks',3);
  assert.equal(doc.apply(map=>alignItems(map,[a,b],'minZ')).ok,true);
  assert.deepEqual(rectOf(doc,a),[-.45,2.45,-.23,2.67]);
  assert.deepEqual(rectOf(doc,b),[.34,2.45,.58,2.64]);
  doc.undo();
  assert.deepEqual(rectOf(doc,a),[-.45,2.7,-.23,2.92]);
});

test('ten thousand objects stay quick to edit, pick and undo',()=>{
  const doc=open(),blocks=[];
  for(let i=0;i<10000;i++){const x=-40+(i%200)*.4,z=-40+Math.floor(i/200)*.4;blocks.push({rect:[x,z,x+.2,z+.2]})}
  doc.setSettings({bounds:[-45,-45,45,45],nav:{cell:.1,clearance:.035}});
  const started=performance.now();
  assert.equal(doc.apply(map=>({...map,blocks:[...map.blocks,...blocks]})).ok,true);
  const ids=doc.map.blocks.slice(-5000).map(block=>block.id);
  doc.begin();
  for(let i=1;i<=60;i++){doc.preview(map=>doc.moved(map,ids,i*.001,0));doc.pick(0,0,.01)}
  doc.commit();doc.undo();doc.redo();
  const elapsed=performance.now()-started;
  assert.equal(doc.map.blocks.length,10004);
  assert.ok(elapsed<1500,`took ${elapsed.toFixed(0)} ms`);
});

// ---- Forts maps: the mode, the zones and the starts' teams ----
// (A Forts map kept for the tests: six metres of counter, the bundled Forts map's layout, which the kitchen grows to hold.)
const fortText=readFileSync(new URL('../tools/network/fixtures/long-fort.json',import.meta.url),'utf8');
const openFort=()=>{const doc=new Doc();doc.load(fortText);return doc};
const problems=doc=>doc.analysis.issues.map(issue=>issue.code);

test('a Forts map loads, saves back byte for byte and its zones are objects like any other',()=>{
  const doc=openFort(),ids=[...doc.items()].map(entry=>entry.item.id);
  assert.equal(doc.toJSON(),fortText);
  // Fourteen pickups: five guns a side, a syrup rifle a side and a box of ammunition in each base.
  assert.equal(ids.length,6+4+14+5+3+4+30+4);
  assert.deepEqual(doc.map.pickups.filter(pickup=>pickup.weapon===0).map(({id,...pickup})=>pickup),[{p:[-2.78,2.42],weapon:0},{p:[2.78,2.42],weapon:0}]);
  assert.equal(new Set(ids).size,ids.length);
  assert.deepEqual([doc.map.mode,doc.map.build,doc.map.zones.map(zone=>zone.kind)],['forts',150,['base','base','field','bacon']]);
  const bacon=doc.find(idOf(doc,'zones',3)),field=doc.find(idOf(doc,'zones',2));
  assert.deepEqual([bacon.kind,bacon.index,bacon.item.p],['zones',3,[0,3.1]]);
  assert.deepEqual([field.kind,field.index,field.item.rect],['zones',2,[-.04,2.2,.04,4]]);
  assert.deepEqual(problems(doc),[]);
  assert.equal(doc.level.fort.bacon[1],3.1);
});

test('zones are moved, resized, retitled, duplicated and removed with undo, and a bad one is refused',()=>{
  const doc=openFort(),base=idOf(doc,'zones',0),bacon=idOf(doc,'zones',3);
  assert.equal(doc.move([base,bacon],.1,-.1).ok,true);
  assert.deepEqual([rectOf(doc,base),doc.find(bacon).item.p],[[-2.9,2.6,-2.4,3.4],[.1,3]]);
  assert.equal(doc.patch(base,{rect:[-2.9,2.6,-2.2,3.4],team:'ketchup'}).ok,true);
  assert.deepEqual(doc.find(base).item,{id:base,kind:'base',team:'ketchup',rect:[-2.9,2.6,-2.2,3.4]});
  const refused=doc.patch(base,{team:'mayo'});
  assert.equal(refused.ok,false);
  assert.match(refused.error,/Zone 1 team/);
  doc.undo();doc.undo();
  assert.equal(doc.toJSON(),fortText);
  const copy=doc.duplicate([bacon],.2,0);
  assert.deepEqual(doc.find(copy.ids[0]).item.p,[.2,3.1]);
  assert.deepEqual(problems(doc),['zone-extra']);
  assert.equal(doc.remove([idOf(doc,'zones',2)]).ok,true);
  assert.deepEqual(doc.map.zones.map(zone=>zone.kind),['base','base','bacon','bacon']);
  assert.ok(problems(doc).includes('field-missing'));
  const added=doc.add('zones',{kind:'field',rect:[-.05,2.2,.05,4]});
  assert.equal(added.ok,true);
  assert.deepEqual(doc.selection,[added.id]);
  assert.equal(doc.add('zones',{kind:'moat',rect:[0,0,1,1]}).ok,false);
  const many=new Doc();many.load(fortText);
  for(let i=0;i<12;i++)assert.equal(many.add('zones',{kind:'bacon',p:[i/10,3]}).ok,true);
  assert.match(many.add('zones',{kind:'bacon',p:[0,3]}).error,/limit is 16/);
});

test('a start is given a team, changes it and gives it up',()=>{
  const doc=openFort(),start=idOf(doc,'starts',0);
  assert.equal(doc.find(start).item.team,'mustard');
  doc.patch(start,{team:'ketchup'});
  assert.deepEqual(problems(doc),['start-wrong-side','team-few-starts']);
  doc.patch(start,{team:null});
  assert.equal('team' in doc.find(start).item,false);
  assert.equal(doc.toJSON().includes('{"p": [-2.75, 2.9], "yaw": -1.5708}'),true);
  assert.deepEqual(problems(doc),['start-teamless','team-few-starts']);
  doc.undo();doc.undo();
  assert.equal(doc.toJSON(),fortText);
});

test('the mode and the build time are settings; a map made a Forts map is given its zones and its starts their teams',()=>{
  const doc=open();
  assert.equal(doc.map.mode,'siege');
  assert.equal(doc.setSettings({mode:'forts'}).ok,true);
  assert.deepEqual([doc.map.mode,doc.map.build],['forts',150]);
  assert.deepEqual(doc.map.zones.map(({id,...zone})=>zone),[{kind:'base',team:'mustard',rect:[-1.8,2.27,-1.35,3.63]},{kind:'base',team:'ketchup',rect:[1.35,2.27,1.8,3.63]},{kind:'field',rect:[-.03,2.27,.03,3.63]},{kind:'bacon',p:[0,2.95]}]);
  assert.deepEqual(doc.map.starts.map(start=>start.team),['ketchup','ketchup','mustard','ketchup','mustard']);
  // The starts keep their ids, and what is left to do is to add starts and something to build with.
  assert.deepEqual(doc.map.starts.map(start=>start.id),normalizeIds(open(),'starts'));
  assert.deepEqual(problems(doc),['team-few-starts','stock-missing']);
  assert.equal(doc.setSettings({build:60}).ok,true);
  assert.equal(doc.map.build,60);
  assert.equal(doc.setSettings({build:1}).ok,false);
  assert.match(doc.toJSON(),/"mode": "forts",\n  "build": 60,\n  "bounds"/);
  // Back to a siege: the zones and teams stay, as layout. With the whole Forts layout the map can still be played as Forts (the mode is the game's),
  // so the analysis still says what Forts would need.
  doc.setSettings({mode:'siege'});
  assert.deepEqual(['build' in doc.map,doc.map.zones.length,problems(doc)],[false,4,['team-few-starts','stock-missing']]);
  doc.setSettings({mode:'forts'});
  assert.deepEqual([doc.map.build,doc.map.zones.length],[150,4]);
  for(let i=0;i<4;i++)doc.undo();
  assert.equal(doc.toJSON(),text);
  // A blank map: its one start is on the ketchup side, so the mustard team is given one in its base, facing the field.
  const fresh=new Doc();
  fresh.setSettings({mode:'forts'});
  assert.deepEqual(fresh.map.starts.map(({id,...start})=>start),[{p:[.83,3.23],yaw:.9,team:'ketchup'},{p:[-1.575,2.95],yaw:-1.5708,team:'mustard'}]);
  // (Two starts: a siege or Free For All on it, which every map can be played as, would have its five hotdogs share them.)
  assert.deepEqual(problems(fresh),['team-few-starts','team-few-starts','few-starts','stock-missing','stock-missing']);
  assert.deepEqual(analyze(compile(normalize(fresh.toJSON()))).issues.map(issue=>issue.code),problems(fresh));
  assert.equal(fortified(normalize(fortText)).zones.length,4);
});

test('a base is picked under everything on it, and a marquee takes an area only when it holds all of it',()=>{
  const doc=openFort(),base=idOf(doc,'zones',0),field=idOf(doc,'zones',2);
  assert.equal(doc.pick(-2.75,2.9,.005),idOf(doc,'starts',0));
  assert.equal(doc.pick(-2.6,2.75,.005),base);
  assert.equal(doc.pick(0,3.1,.005),idOf(doc,'zones',3));
  assert.equal(doc.pick(0,2.5,.005),field);
  // On the deck in the middle the deck wins over the field under it.
  assert.equal(doc.pick(.02,3.25,.005),idOf(doc,'decks',0));
  // Bare counter outside the base; a napkin, 45 cm square, reaches over the counter beside it.
  assert.equal(doc.pick(-1.5,2.3,.005),null);
  assert.equal(doc.pick(-2.2,2.3,.005),doc.map.props.find(prop=>prop.kind==='napkin').id);
  const starts=doc.pickAll([-2.9,2.8,-2.6,3.4]);
  assert.deepEqual(starts.map(id=>doc.find(id).kind),['starts','starts','starts']);
  assert.ok(doc.pickAll([-3.1,2.6,-2.4,3.6]).includes(base));
  assert.equal(doc.pickAll([-.5,2.3,.5,3.9]).includes(field),false);
  assert.ok(doc.snapEdges().x.includes(-2.5)&&doc.snapEdges().x.includes(.04));
  assert.deepEqual(itemBox(doc.find(field).item),[-.04,2.2,.04,4]);
});

// ---- Pickups: guns and boxes of ammunition, the time a taken one takes to come back, and the loose objects' sizes ----
test('an ammo box is a pickup without a count; it is made a gun and a box again, with undo, and saves canonically',()=>{
  const doc=open();
  // What the pickup tool and the inspector's Holds field put in: a box has no count, a gun its full load.
  assert.deepEqual(holding(AMMO_BOX),{weapon:0});
  assert.deepEqual(holding(6),{weapon:6,ammo:60});
  const {ok,id}=doc.add('pickups',{p:[0,3],...holding(AMMO_BOX)});
  assert.equal(ok,true);
  assert.deepEqual(doc.find(id).item,{id,p:[0,3],weapon:0});
  assert.ok(doc.toJSON().includes('\n    {"p": [0, 3], "weapon": 0}\n'));
  // A count given to a box is dropped.
  const counted=doc.add('pickups',{p:[.1,3],weapon:0,ammo:12});
  assert.deepEqual(doc.find(counted.id).item,{id:counted.id,p:[.1,3],weapon:0});
  doc.undo();
  assert.equal(doc.patch(id,holding(6)).ok,true);
  assert.deepEqual(doc.find(id).item,{id,p:[0,3],weapon:6,ammo:60});
  assert.equal(doc.patch(id,{ammo:30}).ok,true);
  assert.equal(doc.patch(id,holding(AMMO_BOX)).ok,true);
  assert.deepEqual(doc.find(id).item,{id,p:[0,3],weapon:0});
  const saved=doc.toJSON();
  assert.ok(saved.includes('\n    {"p": [0, 3], "weapon": 0}\n'));
  assert.equal(/"weapon": 0, "ammo"/.test(saved),false);
  // What Download writes is canonical: the map library reads it and writes it back unchanged.
  assert.equal(stringify(normalize(saved)),saved);
  assert.deepEqual(normalize(saved).pickups.at(-1),{p:[0,3],weapon:0});
  // A gun's count is the game's rules' (kitchen-sim magazine.rs PICKUP): a box made a gun without one is a gun without a count. A weapon that does not
  // exist is refused.
  assert.equal(doc.patch(id,{weapon:2}).ok,true);
  assert.deepEqual(doc.find(id).item,{id,p:[0,3],weapon:2});
  doc.undo();
  assert.equal(doc.patch(id,{weapon:8,ammo:5}).ok,false);
  assert.deepEqual(doc.find(id).item,{id,p:[0,3],weapon:0});
  doc.undo();
  assert.deepEqual(doc.find(id).item,{id,p:[0,3],weapon:6,ammo:30});
  doc.undo();doc.undo();
  assert.deepEqual(doc.find(id).item,{id,p:[0,3],weapon:0});
  doc.undo();
  assert.equal(doc.find(id),null);
  assert.equal(doc.toJSON(),text);
  for(let i=0;i<4;i++)doc.redo();
  assert.equal(doc.toJSON(),saved);
});

test('how long a taken pickup takes to come back is a setting the map leaves out until it is changed',()=>{
  const doc=open();
  assert.deepEqual(RESPAWN,[20,12]);
  assert.equal('respawn' in doc.map,false);
  assert.equal(doc.toJSON().includes('respawn'),false);
  assert.equal(doc.setSettings({respawn:[30,8]}).ok,true);
  assert.deepEqual(doc.map.respawn,[30,8]);
  const saved=doc.toJSON();
  assert.match(saved,/"mode": "siege",\n  "respawn": \[30, 8\],\n  "bounds"/);
  assert.equal(stringify(normalize(saved)),saved);
  // Other settings keep it, and a map opened with it saves it back byte for byte.
  assert.equal(doc.setSettings({name:'Quick counter'}).ok,true);
  assert.deepEqual(doc.map.respawn,[30,8]);
  const reopened=new Doc();reopened.load(saved);
  assert.equal(reopened.toJSON(),saved);
  // Each time is 1 to 600 seconds, and there are two of them.
  assert.match(doc.setSettings({respawn:[0,8]}).error,/respawn\[0\] must be between 1 and 600/);
  assert.equal(doc.setSettings({respawn:[30,601]}).ok,false);
  assert.equal(doc.setSettings({respawn:[30]}).ok,false);
  assert.deepEqual(doc.map.respawn,[30,8]);
  // null takes it out: the map is the one it was, under its new name.
  assert.equal(doc.setSettings({respawn:null}).ok,true);
  assert.equal('respawn' in doc.map,false);
  assert.equal(doc.toJSON(),text.replace('"Counter Siege"','"Quick counter"'));
  doc.undo();
  assert.deepEqual(doc.map.respawn,[30,8]);
  doc.undo();doc.undo();
  assert.equal(doc.toJSON(),text);
  assert.equal(doc.canUndo,false);
  // On a Forts map it comes after the build time, and a change of mode keeps it.
  const fort=openFort();
  assert.equal('respawn' in fort.map,false);
  assert.equal(fort.setSettings({respawn:[25,10]}).ok,true);
  assert.match(fort.toJSON(),/"build": 150,\n  "respawn": \[25, 10\],\n  "bounds"/);
  fort.setSettings({mode:'siege'});
  assert.deepEqual([fort.map.mode,fort.map.respawn],['siege',[25,10]]);
});

test('a loose object takes up on the plan what the physics makes of it: a napkin is 45 cm square, a cutting board 45 cm by 30',()=>{
  assert.deepEqual([FOOTPRINT.napkin,FOOTPRINT.board],[[.225,.225],[.225,.15]]);
  assert.deepEqual(Object.keys(FOOTPRINT),LOOSE);
  for(const [kind,footprint] of Object.entries(FOOTPRINT)){
    const parts=PRESETS[kind]().parts,round=v=>+v.toFixed(6);
    const half=axis=>Math.max(...parts.map(part=>part.type==='lathe'?Math.max(...part.profile.map(([r])=>r)):Math.abs(part.center[axis])+part.half[axis]));
    assert.deepEqual([round(half(0)),round(half(2))],footprint,kind);
  }
  assert.deepEqual(itemBox({kind:'board',p:[0,3],base:0,yaw:0}),[-.225,2.85,.225,3.15]);
  // Turned a quarter, it is 30 cm along x; a marker is never smaller than the marker size.
  assert.deepEqual(itemBox({kind:'board',p:[0,3],base:0,yaw:Math.PI/2}).map(v=>+v.toFixed(6)),[-.15,2.775,.15,3.225]);
  assert.deepEqual(itemBox({p:[0,3],weapon:0},.03).map(v=>+v.toFixed(6)),[-.03,2.97,.03,3.03]);
  // Of the point objects under the pointer the smallest is picked: a chair or a pickup on a napkin, not the napkin.
  const doc=openFort(),napkin=doc.map.props.find(prop=>prop.kind==='napkin'&&prop.yaw);
  assert.deepEqual(napkin.p,[-2.31,3.1]);
  const chair=doc.add('props',{kind:'chair',p:[-2.31,3.2],base:0,yaw:0}).id,pickup=doc.add('pickups',{p:[-2.2,3],weapon:0}).id;
  assert.equal(doc.pick(-2.31,3.21,.005),chair);
  assert.equal(doc.pick(-2.2,3,.005),pickup);
  assert.equal(doc.pick(-2.4,3,.005),napkin.id);
  // Its box decides what a marquee touches.
  assert.equal(doc.pickAll([-2.6,2.8,-2.54,2.9]).includes(napkin.id),false);
  assert.equal(doc.pickAll([-2.6,2.8,-2.53,2.9]).includes(napkin.id),true);
});

// ---- The whole kitchen: sinks, the things on the counter, plank ramps, bins and fitted cabinets (map version 2) ----
const kitchenText=readFileSync(new URL('../assets/maps/the-whole-kitchen.json',import.meta.url),'utf8');
const kitchenFortsText=readFileSync(new URL('../assets/maps/the-whole-kitchen-forts.json',import.meta.url),'utf8');
const versionOf=json=>JSON.parse(json).version;

test('the whole kitchen maps open and save back byte for byte, with their sinks, counter things, planks, bins and cabinets',()=>{
  for(const source of [kitchenText,kitchenFortsText]){
    const doc=new Doc();doc.load(source);
    assert.equal(doc.toJSON(),source);
    assert.equal(doc.dirty,false);
    assert.deepEqual([versionOf(doc.toJSON()),doc.map.cabinets],[2,true]);
    const ids=[...doc.items()].map(entry=>entry.item.id);
    assert.equal(new Set(ids).size,ids.length);
    // Every kind of object is found by its id, a sink and a bin as well. (tools/whole-kitchen.mjs makes these maps, so only what they are made of is checked.)
    const sink=doc.map.sinks[0],bin=doc.map.props.find(prop=>prop.kind==='bin'),[x0,z0,x1,z1]=sink.rect;
    assert.deepEqual(doc.find(sink.id),{kind:'sinks',index:0,item:sink});
    assert.equal(sinkAt(doc.map,(x0+x1)/2,(z0+z1)/2),sink);
    assert.deepEqual(Object.keys(doc.find(bin.id).item),['id','kind','p','yaw']);
    assert.ok(doc.map.ramps.some(ramp=>ramp.base<0&&ramp.style));
    assert.ok(doc.map.blocks.some(block=>KITS.indexOf(block.style)>=2));
    // A bin stands beside the counter, off the map, and that is no problem.
    assert.equal(doc.analysis.issues.some(issue=>issue.code==='outside'),false);
  }
});

test('a sink is drawn, deepened, moved and removed with undo; a map with one is version 2 and without it version 1 again',()=>{
  const doc=open();
  const {ok,id}=doc.add('sinks',{rect:[-.3,2.95,.1,3.25]});
  assert.equal(ok,true);
  assert.deepEqual(doc.find(id).item,{id,rect:[-.3,2.95,.1,3.25],depth:SINK_DEPTH});
  assert.deepEqual(doc.selection,[id]);
  assert.equal(versionOf(doc.toJSON()),2);
  assert.ok(doc.toJSON().includes('"sinks": [\n    {"rect": [-0.3, 2.95, 0.1, 3.25], "depth": 0.18}\n  ],\n  "props"'));
  // Its floor is the ground there, and the analysis says it has no way out.
  assert.equal(doc.level.height(-.1,3.1),-.18);
  assert.deepEqual(doc.analysis.issues.filter(issue=>issue.kind==='sinks').map(issue=>[issue.code,issue.index]),[['sink-trap',0]]);
  assert.equal(sinkAt(doc.map,-.1,3.1),doc.find(id).item);
  assert.equal(sinkAt(doc.map,.5,3.1),null);
  assert.equal(doc.patch(id,{depth:.3}).ok,true);
  assert.equal(doc.find(id).item.depth,.3);
  assert.match(doc.patch(id,{depth:.6}).error,/Sink 1 depth must be between 0.05 and 0.5/);
  assert.equal(doc.move([id],.1,0).ok,true);
  assert.deepEqual(doc.find(id).item.rect,[-.2,2.95,.2,3.25]);
  // Under everything that stands in it: a pickup moved into it is picked first, bare counter in it picks the sink.
  doc.move([idOf(doc,'pickups',0)],.6,.65);
  assert.equal(doc.pick(-.1,3.1,.005),idOf(doc,'pickups',0));
  assert.equal(doc.pick(.1,3.1,.005),id);
  doc.undo();
  // A ramp propped in it, from its floor to the counter, is the way out.
  const ramp=doc.add('ramps',{rect:[-.2,3.05,.1,3.15],rise:'-x',base:-.3,height:.012,style:'wood'});
  assert.equal(ramp.ok,true);
  assert.equal(doc.analysis.issues.some(issue=>issue.code==='sink-trap'),false);
  assert.equal(doc.remove([id,ramp.id]).ok,true);
  assert.equal(doc.toJSON(),text);
  doc.undo();doc.undo();doc.undo();doc.undo();
  assert.deepEqual(doc.find(id).item.rect,[-.3,2.95,.1,3.25]);
  doc.undo();
  assert.equal(doc.find(id),null);
  assert.equal(doc.toJSON(),text);
  assert.equal(doc.canUndo,false);
  // A map holds sixteen sinks at most.
  const many=open();
  for(let i=0;i<16;i++)assert.equal(many.add('sinks',{rect:[-1.7+i*.2,2.3,-1.6+i*.2,2.4]}).ok,true);
  assert.match(many.add('sinks',{rect:[0,3.3,.1,3.4]}).error,/limit is 16/);
});

test('a ramp may be a plank of a material, and start down in a sink; the metal ramp has no style',()=>{
  const doc=open(),ramp=idOf(doc,'ramps',0),before=doc.find(ramp).item;
  assert.equal('style' in before,false);
  assert.equal(doc.patch(ramp,{style:'wood'}).ok,true);
  assert.equal(doc.find(ramp).item.style,'wood');
  assert.equal(versionOf(doc.toJSON()),2);
  assert.equal(doc.patch(ramp,{style:'cheese'}).ok,false);
  // Choosing the metal ramp again sets the style to null, which takes it out.
  assert.equal(doc.patch(ramp,{style:null}).ok,true);
  assert.equal('style' in doc.find(ramp).item,false);
  assert.equal(doc.toJSON(),text);
  // A foot below the counter, down to half a metre.
  assert.equal(doc.patch(ramp,{base:-.18}).ok,true);
  assert.equal(doc.find(ramp).item.base,-.18);
  assert.equal(versionOf(doc.toJSON()),2);
  assert.match(doc.patch(ramp,{base:-.6}).error,/Ramp 1 base must be between -0.5 and 3/);
  assert.equal(doc.patch(ramp,{base:0}).ok,true);
  assert.equal(doc.toJSON(),text);
  // Several objects patched together each take only the fields they have: a metal ramp no material, while planks and a block take one together.
  const both=[ramp,idOf(doc,'ramps',1)];
  assert.equal(doc.patch(both,{style:'stone'}).ok,true);
  assert.equal(doc.toJSON(),text);
  for(const each of both)doc.patch(each,{style:'wood'});
  assert.equal(doc.patch([...both,idOf(doc,'blocks',0)],{style:'dark'}).ok,true);
  assert.deepEqual([...doc.map.ramps.map(each=>each.style),doc.map.blocks[0].style],['dark','dark','dark']);
});

test('the counter things are blocks with a front; a bin stands anywhere, turned by its yaw',()=>{
  const doc=open();
  assert.deepEqual(Object.keys(KIT_NAME),KITS);
  assert.deepEqual([KIT_NAME['knife-block'],KIT_NAME['paper-towel'],KIT_NAME.faucet,KIT_NAME.griddle],['Knife block','Paper towels','Tap','Electric griddle']);
  const block=idOf(doc,'blocks',2);
  assert.equal(doc.patch(block,{style:'kettle'}).ok,true);
  assert.deepEqual(doc.find(block).item,{id:block,rect:[-.45,2.7,-.23,2.92],height:.25,style:'kettle',facing:'-z'});
  assert.equal(versionOf(doc.toJSON()),2);
  assert.equal(doc.patch(block,{facing:'+x'}).ok,true);
  // The appliances the first maps knew keep a map at version 1.
  assert.equal(doc.patch(block,{style:'toaster'}).ok,true);
  assert.equal(versionOf(doc.toJSON()),1);
  doc.undo();doc.undo();doc.undo();
  assert.equal(doc.toJSON(),text);
  // A bin, off the counter beside it: it has no height to sit at, and takes up 34 cm by 30 cm, turned by its yaw.
  const {ok,id}=doc.add('props',{kind:'bin',p:[2.1,3],yaw:-Math.PI/2});
  assert.equal(ok,true);
  assert.deepEqual(doc.find(id).item,{id,kind:'bin',p:[2.1,3],yaw:-1.5708});
  assert.ok(doc.toJSON().includes('\n    {"kind": "bin", "p": [2.1, 3], "yaw": -1.5708}\n'));
  assert.equal(versionOf(doc.toJSON()),2);
  assert.deepEqual(FIXED.bin,[.17,.15]);
  assert.deepEqual(itemBox(doc.find(id).item).map(v=>+v.toFixed(4)),[1.95,2.83,2.25,3.17]);
  assert.equal(doc.pick(2.2,3.1,.005),id);
  assert.deepEqual(doc.pickAll([2.2,2.9,2.3,3]),[id]);
  assert.equal(doc.analysis.issues.some(issue=>issue.code==='outside'),false);
  assert.equal(doc.patch(id,{yaw:0}).ok,true);
  assert.deepEqual(itemBox(doc.find(id).item).map(v=>+v.toFixed(4)),[1.93,2.85,2.27,3.15]);
  assert.equal(doc.move([id],.5,-.25).ok,true);
  assert.deepEqual(doc.find(id).item.p,[2.6,2.75]);
  // A base given to a bin is dropped: it stands on the kitchen floor.
  const based=doc.add('props',{kind:'bin',p:[-2.1,3],base:.2,yaw:0});
  assert.deepEqual(doc.find(based.id).item,{id:based.id,kind:'bin',p:[-2.1,3],yaw:0});
  const copies=doc.duplicate([id],.1,0);
  assert.deepEqual(doc.find(copies.ids[0]).item.p,[2.7,2.75]);
  assert.equal(doc.remove([id,based.id,copies.ids[0]]).ok,true);
  assert.equal(doc.toJSON(),text);
});

test('fitted cabinets are a setting a map leaves out until it is on, and turning them on makes it version 2',()=>{
  const doc=open();
  assert.equal('cabinets' in doc.map,false);
  assert.equal(doc.setSettings({cabinets:true}).ok,true);
  assert.equal(doc.map.cabinets,true);
  const saved=doc.toJSON();
  assert.match(saved,/^\{\n  "format": "hotdog-map",\n  "version": 2,\n/);
  assert.match(saved,/"mode": "siege",\n  "cabinets": true,\n  "bounds"/);
  assert.equal(stringify(normalize(saved)),saved);
  const reopened=new Doc();reopened.load(saved);
  assert.equal(reopened.toJSON(),saved);
  // Other settings keep them; turning them off leaves them out again, and the map is version 1 as it was.
  assert.equal(doc.setSettings({theme:'night'}).ok,true);
  assert.equal(doc.map.cabinets,true);
  assert.equal(doc.setSettings({cabinets:false}).ok,true);
  assert.equal('cabinets' in doc.map,false);
  assert.equal(doc.toJSON(),text.replace('"theme": "kitchen"','"theme": "night"'));
  assert.equal(doc.setSettings({cabinets:'yes'}).ok,false);
  doc.undo();
  assert.equal(doc.map.cabinets,true);
  doc.undo();doc.undo();
  assert.equal(doc.toJSON(),text);
  assert.equal(doc.canUndo,false);
  // The whole kitchen without its cabinets, sink, planks, counter things and bin is a version 1 map.
  const kitchen=new Doc();kitchen.load(kitchenText);
  assert.equal(kitchen.setSettings({cabinets:false}).ok,true);
  assert.equal(versionOf(kitchen.toJSON()),2);
  const newer=[...kitchen.items()].filter(({kind,item})=>kind==='sinks'||item.kind==='bin'||(kind==='ramps'&&(item.base<0||item.style))||(kind==='blocks'&&KITS.indexOf(item.style)>=2)).map(({item})=>item.id);
  assert.equal(kitchen.remove(newer).ok,true);
  const plain=kitchen.toJSON();
  assert.equal(versionOf(plain),1);
  assert.equal(/sinks|cabinets|"bin"|faucet/.test(plain),false);
  assert.equal(stringify(normalize(plain)),plain);
});
