// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Titan's Kitchen (assets/maps/titans-kitchen.json): the kitchen FLOOR at night, the arena of Titan mode, where one giant hotdog 1.8 m tall
// (kitchen-sim titan.rs: twelve hotdogs tall, 0.36 m round) hunts the 15 cm hotdogs. The same kitchen as the counter maps, seen from the floor and at
// true scale: the counter's cabinets (0.9025 m, the counter maps' counter top) along the back wall with a toe-kick gap under them, an oven between
// them, the fridge on the front wall with its door wide open, a table and four chairs, a sideboard on legs, a bar stool, and in the front corners the
// pet's corner (a cardboard box with the pet's carpeted ramp up to it, its bowls at the foot) and a step stool with a cookbook dropped against it and a
// tea towel hanging down it.
// It is laid out here and checked with the game's own map library (map.mjs: `analyze` must find nothing wrong) before anything is written.
//
//   node tools/titans-kitchen.mjs            checks it and says what it holds
//   node tools/titans-kitchen.mjs --write    and writes it, in canonical form
//
// How the format says it (interface of the Titan mode, .superpowers/sdd/adhoc/titan/interface.md):
//   - `"mode": "titan"`; `starts[0]` is the titan's (in the middle, well clear of everything), the rest the small hotdogs' (round the edge).
//   - `"floor": true`: the game draws tiles and a room whose walls stand at the bounds (heights still count from map FLOOR).
//   - Blocks are solid to everyone: table, chair and sideboard legs (pillars), the cabinets behind their toe-kick, the fridge and its open door, the
//     oven, the bowls.
//   - Box props with a base are the dens: scenery to small hotdogs (they walk under them) and walls to the titan. The table top (0.72 m up), the
//     chair seats and backs (0.43 m), the sideboard's body (0.22 m), the bar stool's seat (0.63 m) and the cabinets' fronts over the toe-kick
//     (0.17 m: a small hotdog, 0.15 m, walks in under them) with the worktop's lip.
//   - Decks and ramps are what a small hotdog climbs: the box (0.40 m, knee high to the titan) up the pet ramp; the step stool's lower step (0.20 m)
//     up the cookbook, its top (0.38 m) up the tea towel hanging over the step.
// The room lies away from the counter maps' kitchen (x 0, z -10), so the two never meet.
import {writeFileSync} from 'node:fs';
import {normalize,stringify,compile,analyze,FLOOR} from './network/map.mjs';

const ROOT=new URL('..',import.meta.url).pathname;
const r4=v=>Math.round(v*1e4)/1e4;
export const BOUNDS=[-3.6,-13,3.6,-7];
const [X0,Z0,X1,Z1]=BOUNDS;
// The counter's height (the counter maps' counter top, map FLOOR above the kitchen floor), the toe-kick's height and depth, the cabinets' depth.
export const COUNTER=FLOOR,TOE=[.17,.1],DEPTH=.6;
// The titan's start, in the middle, and its body's radius and height (kitchen-sim titan.rs RADIUS, TALL; map.rs's tests hold these to them).
export const TITAN_START=[.3,-10],TITAN_RADIUS=.36,TITAN_TALL=1.8;
/** A yaw facing from p towards q (yaw 0 looks along -z, a quarter turn left along -x). */
const toward=(p,q)=>r4(Math.atan2(-(q[0]-p[0]),-(q[1]-p[1])));

export function layout(){
  const m={format:'hotdog-map',version:2,name:"Titan's Kitchen",theme:'night',mode:'titan',floor:true,bounds:BOUNDS,nav:{cell:.04,clearance:.035},
    starts:[],spawns:[],pickups:[],blocks:[],decks:[],ramps:[],props:[],zones:[]};
  const block=(rect,height,style,facing)=>m.blocks.push({rect:rect.map(r4),height:r4(height),style,...(facing?{facing}:{})});
  const box=(rect,base,height,style)=>m.props.push({kind:'box',rect:rect.map(r4),base:r4(base),height:r4(height),style});
  const deck=(rect,height,style)=>m.decks.push({rect:rect.map(r4),height:r4(height),style});
  const ramp=(rect,height,rise,style)=>m.ramps.push({rect:rect.map(r4),base:0,height:r4(height),rise,style});
  /** Four legs (blocks) at the corners of `rect`, inset `inset`, `size` square and `height` tall, and more along the long sides every `every` metres. */
  const legs=(rect,size,height,style,inset=.02,every=0)=>{
    const [a,b,c,d]=rect,xs=[a+inset,c-inset-size],zs=[b+inset,d-inset-size];
    if(every){const long=d-b>c-a;const n=Math.floor((long?d-b:c-a)/every);for(let i=1;i<n;i++){const t=i/n;(long?zs:xs).splice(-1,0,(long?b+inset+(d-b-2*inset-size)*t:a+inset+(c-a-2*inset-size)*t))}}
    for(const x of xs)for(const z of zs)block([x,z,x+size,z+size],height,style);
  };

  // ----- The back wall (z = Z1): the base cabinets, 0.6 m deep, the oven between them. Each run: the carcass and plinth (a block, solid to everyone, its
  // top the worktop), the doors standing proud of the plinth by the toe-kick's depth from TOE[0] up (a box: the den), and the worktop's front lip.
  const oven=[.62,1.38];
  for(const [a,b] of [[X0,oven[0]],[oven[1],X1]]){
    const front=Z1-DEPTH;
    block([a,front+TOE[1],b,Z1],COUNTER,'wood');
    box([a,front,b,front+TOE[1]],TOE[0],.86-TOE[0],'wood');
    box([a,front-.02,b,front+TOE[1]],.86,COUNTER-.86,'stone');
  }
  // A freestanding range, flush with the worktop's lip, a little taller for its backguard (the clock glows on it).
  block([oven[0],Z1-DEPTH-.04,oven[1],Z1],.93,'oven','-z');

  // ----- The front wall (z = Z0): the fridge, 1.8 m tall and 0.68 m deep, facing into the room, its door wide open (lit inside: the room's other
  // light). The door is a block of its own, a thin steel one out at a right angle from the hinge at the fridge's +x front corner (src/floor.rs
  // `door_side` knows it), so that what stops a hotdog is what it sees.
  block([-.42,Z0,.42,Z0+.68],1.8,'fridge','+z');
  block([.42,Z0+.68,.48,Z0+.68+.84],1.8,'steel');

  // ----- The left wall (x = X0): a sideboard on legs 0.22 m tall, 1.6 m long and 0.42 m deep: under it is a den, open along its front and at both ends.
  const sideboard=[X0,-11.2,X0+.42,-9.6];
  legs(sideboard,.04,.22,'dark',.02,.8);
  box(sideboard,.22,.64,'dark');

  // ----- The table (1.4 m by 0.85 m, its top 0.72 to 0.755 m up) and four chairs pulled out round it (seats 0.43 m up, backs to 0.92 m). The whole group
  // is a wall to the titan and a den, with gaps all round, to the small hotdogs; its legs are pillars.
  const table=[-2,-10.35,-.6,-9.5];
  legs(table,.06,.72,'wood',.05);
  box(table,.72,.035,'wood');
  for(const x of [-1.65,-.95]){
    for(const side of [1,-1]){
      // North of the table (towards the cabinets, +z) the chair's back is at its +z edge; south, at its -z edge.
      const z0=side>0?table[3]+.05:table[1]-.05-.42,seat=[x-.21,z0,x+.21,z0+.42];
      legs(seat,.035,.43,'wood',.015);
      box(seat,.43,.04,'wood');
      box(side>0?[seat[0],seat[3]-.03,seat[2],seat[3]]:[seat[0],seat[1],seat[2],seat[1]+.03],.47,.45,'wood');
    }
  }

  // ----- A bar stool in the open on the right: four slim legs and a seat 0.63 m up. A pillar to lure a charge into.
  const stool=[1.62,-10.78,1.98,-10.42];
  legs(stool,.03,.63,'metal',.02);
  box(stool,.63,.04,'dark');

  // ----- The pet's corner (front left): a cardboard box against the walls, 0.4 m tall (the titan's knee), its carpeted ramp up to it from the room,
  // and the pet's two bowls by the ramp's foot.
  const crate=[X0+.02,Z0+.02,X0+.52,Z0+.42];
  deck(crate,.4,'card');
  ramp([crate[2],Z0+.07,crate[2]+.75,Z0+.37],.4,'-x','red');
  block([-2.1,Z0+.08,-1.92,Z0+.26],.06,'pet-bowl','+z');
  block([-1.86,Z0+.06,-1.62,Z0+.3],.07,'pet-bowl','+z');

  // ----- The step stool (front right): white plastic, a step 0.2 m up at its front and its top 0.38 m up behind. A big cookbook dropped against the
  // step makes a ramp up to it, and a yellow tea towel left hanging from the top down over the step another, up to the top.
  const steps=[2.96,Z0+.05,3.4,Z0+.49];
  deck(steps,.2,'white');
  deck([steps[0],steps[1],steps[2],steps[1]+.22],.38,'white');
  ramp([3.06,steps[3],3.3,steps[3]+.26],.2,'-z','green');
  m.ramps.push({rect:[3.06,r4(steps[1]+.22),3.3,steps[3]].map(r4),base:.2,height:.38,rise:'-z',style:'yellow'});

  // ----- Who starts where: the titan in the middle, facing the fridge; the small hotdogs round the edge, facing it.
  m.starts.push({p:TITAN_START,yaw:0});
  for(const p of [[-3.1,-8.2],[-1.3,-8.25],[2,-8],[3.15,-9.4],[3.1,-11.6],[1.4,-12.5],[-1.5,-12.4],[-2.9,-11.8]])m.starts.push({p,yaw:toward(p,TITAN_START)});
  // Cat spawn points (a siege may be played here too): in the four corners of the open floor.
  for(const p of [[-2.6,-8],[2.8,-8.2],[2.6,-12],[-2.4,-12.1]])m.spawns.push({p});

  // ----- Pickups: spread so nobody camps. Boxes of ammunition in the dens (under the table, in the toe-kick) and on the box; the sniper on top of the
  // stool and the bazooka in the open in front of the fridge, where reaching them is a risk.
  const pickup=(p,weapon)=>m.pickups.push({p:p.map(r4),weapon});
  pickup([-1.3,-9.92],0);
  pickup([2.4,Z1-DEPTH+.04],0);
  pickup([(crate[0]+crate[2])/2,(crate[1]+crate[3])/2],0);
  pickup([X0+.21,-10.4],1);
  pickup([3.25,-10.3],1);
  pickup([(steps[0]+steps[2])/2,steps[1]+.11],2);
  pickup([-2.6,-10.4],3);
  pickup([.9,-11.3],4);
  pickup([-.3,-7.85],5);
  pickup([2.2,-9.4],6);
  pickup([-.9,-12.1],7);
  return m;
}

/** The map in canonical form, checked: `analyze` must find nothing at all to say. */
export function build(){
  const map=normalize(layout()),{issues,stats}=analyze(compile(map));
  if(issues.length)throw new Error(`Titan's Kitchen has problems:\n${issues.map(i=>`  ${i.severity} ${i.code}: ${i.message}`).join('\n')}`);
  return {map,stats,text:stringify(map)};
}

if(import.meta.url===`file://${process.argv[1]}`){
  const {map,stats,text}=build();
  console.log(`${map.name}: ${(BOUNDS[2]-BOUNDS[0]).toFixed(1)} x ${(BOUNDS[3]-BOUNDS[1]).toFixed(1)} m, ${map.starts.length} starts, ${map.spawns.length} spawns, `+
    `${map.pickups.length} pickups, ${map.blocks.length} blocks, ${map.decks.length} decks, ${map.ramps.length} ramps, ${map.props.length} boxes; `+
    `${stats.reachable} of ${stats.standable} standable cells reachable`);
  if(process.argv.includes('--write')){writeFileSync(`${ROOT}assets/maps/titans-kitchen.json`,text);console.log('wrote assets/maps/titans-kitchen.json')}
}
