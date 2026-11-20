// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The Whole Kitchen: one layout, two maps. The counter (its sink, its things, its ramps and its perches) is laid out once here, and written out as a
// siege (assets/maps/the-whole-kitchen.json: zombie spawn points all over) and as a Forts map (assets/maps/the-whole-kitchen-forts.json: a base at each end,
// the forcefield across the middle, the bacon in the sink), so that the two cannot drift apart. Both are checked with the game's own map library (map.mjs:
// `analyze` must find nothing wrong) before anything is written.
//
//   node tools/whole-kitchen.mjs            checks both and says what they hold
//   node tools/whole-kitchen.mjs --write    and writes them, in canonical form
//
// (The Forts map also has an electric griddle on the centre line, in front of the sink.)
// The counter runs 8.4 m along x, almost the largest kitchen the game builds (map.mjs ROOM), and 2.4 m deep, from the front (z = 2.1, facing the fridge)
// to the back wall. It is laid out mirrored about x = 0: the sink, its ramps, the perches, the pickups and the starts exactly, the things on the counter
// as pairs of the same size where a kitchen would not have two of a thing.
import {writeFileSync} from 'node:fs';
import {normalize,stringify,compile,analyze,fielded,FLOOR,LIP,MARGIN,WEAPONS} from './network/map.mjs';

const ROOT=new URL('..',import.meta.url).pathname;
const r4=v=>Math.round(v*1e4)/1e4;
const BOUNDS=[-4.2,2.1,4.2,4.5];
// The Forts microwaves' height (the turrets stand on top).
const MICROWAVE=.36;
const SINK={rect:[-.36,3.2,.36,3.76],depth:.18};
// Rounds a new pickup holds, by weapon: the editor's full loads. A mayonnaise bottle (7) once the game has one.
import {ROUNDS} from '../editor/model.js';
const MAYO=WEAPONS.length>7;

/** The layout both maps share: blocks, decks, ramps and the sink, mirrored about x = 0. (In Forts the microwaves are full-size ones, `forts`.) */
function counter(m,{forts=false}={}){
  const block=(rect,height,style,facing)=>m.blocks.push({rect:rect.map(r4),height,style,...(facing?{facing}:{})});
  const both=(rect,height,style,facing,other=style,otherFacing=facing&&{'+x':'-x','-x':'+x'}[facing]||facing)=>{
    block(rect,height,style,facing);
    block([-rect[2],rect[1],-rect[0],rect[3]],height,other,otherFacing);
  };
  const ramp=(rect,base,height,rise,style)=>m.ramps.push({rect:rect.map(r4),base,height:r4(height),rise,...(style?{style}:{})});
  // The sink: a basin 72 by 56 cm and 18 cm deep in the middle of the counter, the tap behind it, and a chopping board propped in it from each side, its
  // top on the rim: the way out for anyone who drops in (a plain jump is 4 cm short; a stimmed one clears it).
  m.sinks.push({...SINK});
  const [sx0,sz0,sx1,sz1]=SINK.rect,lane=[3.42,3.54];
  ramp([sx0,lane[0],-.09,lane[1]],-SINK.depth,.012,'-x','wood');
  ramp([.09,lane[0],sx1,lane[1]],-SINK.depth,.012,'+x','wood');
  block([-.035,sz1+.05,.035,sz1+.12],.34,'faucet','-z');
  // A microwave in each back corner, and a plank up to its top from the side: a perch over each end. A ramp onto a block has to stand clear of the block's
  // edge by the time it meets the margin bodies keep from it: its top is the block's, plus LIP, over the run less MARGIN.
  // (In Forts a full-size countertop microwave, 62 cm wide, 48 deep and 36 tall, against the back wall, at a hotdog's true scale; its plank is longer, so
  // that it climbs no more steeply.)
  const micro=forts?[-4.17,4.02,-3.55,4.5]:[-4.12,4.06,-3.6,4.44],mh=forts?MICROWAVE:.27,run=forts?.546:.42,plank=r4((mh+LIP+.003)*run/(run-MARGIN));
  both(micro,mh,'microwave','-z');
  ramp([micro[2],4.1,micro[2]+run,4.24],0,plank,'-x','wood');
  ramp([-micro[2]-run,4.1,-micro[2],4.24],0,plank,'+x','wood');
  // The breakfast end (mustard's, at -x) and the coffee end (ketchup's), against the back wall: tall boxes of cereal opposite a row of canisters, the toaster
  // opposite the kettle.
  block([-3.44,4.3,-3.22,4.38],.3,'cereal','-z');block([-3.18,4.27,-2.98,4.35],.27,'cereal','-z');
  block([3.22,4.27,3.34,4.39],.25,'canister','-z');block([3.37,4.29,3.48,4.4],.2,'canister','-z');block([3.06,4.3,3.16,4.4],.15,'canister','-z');
  both([-2.72,4.18,-2.42,4.36],.2,'toaster','-z','kettle');
  // A roll of paper towels on its stand at each end of the middle, the knife block and the spice rack against the wall.
  both([-2.26,3.86,-2.14,3.98],.28,'paper-towel','-z');
  block([-1.9,4.18,-1.78,4.36],.22,'knife-block','-z');
  block([1.95,4.36,2.31,4.45],.2,'spice-rack','-z');
  // A wooden shelf riser against the wall each side, a plank up to it: a raised spot behind cover (the jars), for lobbing mayonnaise. (Higher than
  // a bot's step up without a ramp, CLIMB in map.mjs, so that bots take the plank.)
  m.decks.push({rect:[-1.75,4.02,-1.3,4.42],height:.14,style:'wood'},{rect:[1.3,4.02,1.75,4.42],height:.14,style:'wood'});
  ramp([-1.75,3.62,-1.57,4.02],0,.14,'+z','wood');ramp([1.57,3.62,1.75,4.02],0,.14,'+z','wood');
  // Jars and canisters to climb: a jar (10.5 cm: a plain jump) beside a canister (17 cm: from the jar), each side.
  both([-1.42,3.62,-1.33,3.71],.105,'jar','-z');
  both([-1.3,3.66,-1.19,3.77],.17,'canister','-z','jar');
  both([-1.16,3.71,-1.07,3.8],.1,'jar','-z');
  // Low cover in the middle of each half: a bowl of fruit opposite a squat cookie jar; beside the sink the dish rack, and opposite
  // it a big bowl of fruit.
  both([-2.9,3.38,-2.64,3.64],.11,'fruit-bowl','-z','jar');
  both([-1.02,3.84,-.6,4.2],.15,'dish-rack','-z','fruit-bowl');
  // Cutting boards to stand on, near the front: a step up, not a climb.
  m.decks.push({rect:[-2.02,2.52,-1.46,2.88],height:.02,style:'wood'},{rect:[1.46,2.52,2.02,2.88],height:.02,style:'wood'});
  // Cover across the middle and the front of each half: canisters and jars, and a box of cereal left out after breakfast.
  both([-2.46,2.98,-2.3,3.14],.22,'canister','-z','jar');
  both([-1.84,3.26,-1.64,3.46],.18,'canister','-z','canister');
  both([-.85,2.55,-.66,2.74],.16,'canister','-z','canister');
  both([-1.32,2.66,-1.12,2.73],.3,'cereal','-z','cereal');
  // Forts: an electric griddle across the middle, 50 by 28 cm and 6 cm high, its thermostat dial on its side towards the middle of the counter (on the centre line, so
  // that it is as near one team as the other), clear of the pickup at the front (the dial is 0.6 m from its pad: a hotdog at the one is not within reach of the other).
  // A hotdog jumps onto the plate (6 cm is more than a step), and burns on it when it is hot (crates/kitchen-sim/src/griddle.rs).
  if(forts)block([-.25,2.62,.25,2.9],.06,'griddle','+z');
}

/** Pickups, mirrored: each side a pistol by the base, a sniper up by the perch, hot sauce and a relish glob in the middle, a syrup rifle by the sink. */
function pickups(m,{sink}){
  const add=(x,z,weapon)=>m.pickups.push(weapon?{p:[r4(x),r4(z)],weapon,ammo:ROUNDS[weapon]}:{p:[r4(x),r4(z)],weapon});
  for(const s of [-1,1]){
    add(s*3.3,2.35,1);add(s*3.0,4.05,2);add(s*2.35,3.3,3);add(s*1.45,2.35,4);add(s*.75,3.1,6);add(s*2.1,2.45,0);add(s*1.95,3.0,1);
    // The mayonnaise (which lobs over cover) up on the riser.
    if(MAYO)add(s*1.45,4.25,7);
  }
  add(0,2.3,5);
  if(sink)add(0,3.48,4);
}

/** The loose things on the counter: pots and trays to knock about in the siege; in Forts each side a builder's stock of what a kitchen counter has, at its
 * true size beside a 15 cm hotdog and set out tidily: three cutting boards in a pile, a bread bin with two tins on it, two napkins folded flat, a row of three
 * cereal boxes, two more tins, five plates in a pile with a bowl on top, two rolls of paper towels, three mugs, two salt shakers and a pepper shaker, and a
 * cereal bowl (crates/kitchen-sim/src/props.rs has their shapes). */
function loose(m,{stock}){
  const put=(kind,x,z,yaw=0,base=0)=>m.props.push({kind,p:[r4(x),r4(z)],base:r4(base),yaw:r4(yaw)});
  for(const s of [-1,1]){
    if(stock){
      const pile=(kind,x,z,n,step)=>{for(let i=0;i<n;i++)put(kind,s*x,z,0,i*step)};
      pile('board',2.75,2.4,3,.025);
      put('breadbox',s*3.3,3.87);put('tin',s*3.4,3.87,0,.2);put('tin',s*3.2,3.87,0,.2);
      pile('napkin',2.25,3.6,2,.008);
      for(let i=0;i<3;i++)put('cereal',s*2.75,2.72+i*.24,s*-1.5708);
      pile('tin',1.45,3.05,2,.11);
      pile('plate',2.6,3.98,5,.02);put('dish',s*2.6,3.98,0,.1);
      put('roll',s*.48,4.35);put('roll',s*.34,4.35);
      for(let i=0;i<3;i++)put('cup',s*(1.2-i*.11),4.38,s*1.5708);
      put('salt',s*.86,4.38);put('salt',s*.805,4.38);put('pepper',s*.75,4.38);
      put('dish',s*.5,2.5);
    }else{
      // (No tray in Forts: a builder walking up to one shoves it along the counter, and could shove it into the sink.)
      put('bowl',s*2.45,2.75);put('tray',s*1.55,3.05,s*.3);put('crate',s*.62,2.3);put('crate',s*2.6,3.95);
      put('chair',s*2.95,2.75,s*1.2);put('pillow',s*2.2,3.6,.4);put('crate',s*3.3,3.1);put('crate',s*3.42,3.1);put('napkin',s*3.75,2.45,0);
    }
  }
  // The bin, on the kitchen floor past the coffee end of the counter.
  m.props.push({kind:'bin',p:[4.56,3.05],yaw:0});
}

function base(name,mode){
  return {format:'hotdog-map',version:2,name,theme:'kitchen',mode,...(mode==='forts'?{build:150}:{}),cabinets:true,bounds:BOUNDS,nav:{cell:.04,clearance:.035},
    starts:[],spawns:[],pickups:[],blocks:[],decks:[],ramps:[],sinks:[],props:[],zones:[]};
}

function siege(){
  const m=base('The Whole Kitchen','siege');
  counter(m);
  for(const [x,z,yaw] of [[-.9,2.75,0],[.9,2.75,0],[-2.4,2.4,0],[2.4,2.4,0],[0,2.45,0]])m.starts.push({p:[x,z],yaw:r4(yaw+Math.PI)});
  // Zombie spawn points all over the counter: two by each microwave, the corners at the front, round the sink, and along the middle.
  for(const s of [-1,1])for(const [x,z] of [[3.35,3.75],[3.9,2.95],[2.0,3.95],[1.05,2.25],[.62,2.95],[2.75,3.25]])m.spawns.push({p:[r4(s*x),z]});
  pickups(m,{sink:true});
  loose(m,{stock:false});
  for(const s of [-1,1])m.props.push({kind:'turret',p:[r4(s*3.86),4.32],base:r4(.27+.0175),yaw:0});
  return m;
}

function forts(){
  const m=base('The Whole Kitchen (Forts)','forts');
  counter(m,{forts:true});
  const teams=['mustard','ketchup'];
  for(const [t,s] of [[0,-1],[1,1]]){
    for(const z of [2.95,3.2,3.45])m.starts.push({p:[r4(s*3.85),z],yaw:s<0?-1.5708:1.5708,team:teams[t]});
    m.zones.push({kind:'base',team:teams[t],rect:s<0?[-4.2,2.75,-3.55,3.7]:[3.55,2.75,4.2,3.7]});
  }
  m.zones.push({kind:'field',rect:[-.04,BOUNDS[1],.04,BOUNDS[3]]},{kind:'bacon',p:[0,3.48]});
  // Cats only come out in a stalemate: they well up at the spawn points round the middle.
  for(const [x,z] of [[-1.2,2.3],[1.2,2.3],[-1.0,3.0],[1.0,3.0]])m.spawns.push({p:[x,z]});
  pickups(m,{sink:false});
  loose(m,{stock:true});
  for(const s of [-1,1])m.props.push({kind:'turret',p:[r4(s*3.86),4.32],base:r4(MICROWAVE+.0175),yaw:0});
  return m;
}

const write=process.argv.includes('--write');
let bad=false;
for(const [file,make] of [['the-whole-kitchen',siege],['the-whole-kitchen-forts',forts]]){
  const map=normalize(make()),level=compile(map),{issues,stats}=analyze(level);
  const text=stringify(map);
  console.log(`${file}: ${map.mode}, version ${map.version}, ${r4(map.bounds[2]-map.bounds[0])} x ${r4(map.bounds[3]-map.bounds[1])} m; starts ${stats.starts}, spawns ${stats.spawns}, pickups ${stats.pickups}, blocks ${stats.blocks}, decks ${stats.decks}, ramps ${stats.ramps}, sinks ${stats.sinks}, props ${stats.props}; ${stats.cells} cells, ${stats.reachable} reachable, ${stats.components} zombie regions; ${text.length} bytes`);
  for(const issue of issues){console.log(`  ${issue.severity}: ${issue.code} ${issue.kind}#${issue.index+1}: ${issue.message}`);if(issue.severity==='error')bad=true}
  if(map.mode==='forts'){const up=fielded(map);console.log(`  forcefield up: bacon clear ${up.clear(...map.zones.find(z=>z.kind==='bacon').p)}`)}
  if(write&&!bad)writeFileSync(`${ROOT}assets/maps/${file}.json`,text);
}
if(bad)process.exitCode=1;
