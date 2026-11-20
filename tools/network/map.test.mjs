// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Shared map library: format, collision, navigation and analysis. Run with `node --test tools/network/map.test.mjs`.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,existsSync,readdirSync} from 'node:fs';
import {PRESETS} from './presets.mjs';
import {FLOOR,RANGE,MapError,MODES,TEAMS,TEAM_NAMES,ZONES,BUILD,KINDS,LIMITS,LOOSE,WEAPONS,AMMO_BOX,RESPAWN,normalize,normalizeItem,stringify,compile,analyze,gridHash,groundHash,fielded,fortress,fortLayout,modes,ROOM,KITS,FIRST_KITS,STYLES,FIRST_STYLES,ROUND,round} from './map.mjs';

const read=name=>readFileSync(new URL(name,import.meta.url),'utf8');
const text=read('../../assets/maps/counter-siege.json');
const base=()=>JSON.parse(text);
const edit=change=>{const map=base();change(map);return map};
const level=()=>compile(normalize(base()));
// Deterministic sampling keeps failures reproducible.
const random=seed=>()=>{seed=seed+0x6D2B79F5|0;let t=Math.imul(seed^seed>>>15,1|seed);t=t+Math.imul(t^t>>>7,61|t)^t;return((t^t>>>14)>>>0)/4294967296};

// The rules exactly as the game hard-coded them before maps became data. The default map must reproduce them.
const obstacles=[[-1.46,3.10,-.96,3.43],[1.29,3.17,1.66,3.43],[-.45,2.70,-.23,2.92],[.34,2.45,.58,2.64]];
const legacyClear=(x,z,r=.035)=>x>=-1.8&&x<1.8&&z>=2.27&&z<3.63&&!obstacles.some(o=>x>=o[0]-r&&x<=o[2]+r&&z>=o[1]-r&&z<=o[3]+r);
const legacyGround=(x,z)=>{if(x>=1.25&&x<=1.76&&z>=3.16&&z<=3.61)return FLOOR+.28;if(x>=.55&&x<=1.25&&z>=3.45&&z<=3.61)return FLOOR+(x-.55)/.70*.28;if(x>=1.55&&x<=1.75&&z>=2.46&&z<=3.16)return FLOOR+(z-2.46)/.70*.28;return FLOOR};
const legacyCanStep=(a,b)=>{const h=legacyGround(b[0],b[2]);return h<=a[1]+.045&&b[0]>=-1.8&&b[0]<1.8&&b[2]>=2.27&&b[2]<3.63&&obstacles.every(o=>Math.max(h,b[1])>=FLOOR+.255||b[0]<o[0]-.03||b[0]>o[2]+.03||b[2]<o[1]-.03||b[2]>o[3]+.03)};
const legacyBox=(o,d,min,max)=>{let near=0,far=Infinity;for(let i=0;i<3;i++){if(Math.abs(d[i])<1e-8){if(o[i]<min[i]||o[i]>max[i])return Infinity}else{const a=(min[i]-o[i])/d[i],b=(max[i]-o[i])/d[i];near=Math.max(near,Math.min(a,b));far=Math.min(far,Math.max(a,b));if(near>far)return Infinity}}return near};
const legacyCover=(o,d)=>Math.min(12,...obstacles.map(b=>legacyBox(o,d,[b[0],FLOOR,b[1]],[b[2],FLOOR+.25,b[3]])));

test('the bundled map is stored in canonical form',()=>{
  const map=normalize(base());
  // (Every list is there once normalised, a map's sinks too: its file leaves them out when there are none.)
  assert.deepEqual(map,{...base(),sinks:[]});
  assert.equal(stringify(map),text);
});

test('normalising fills optional fields and trims coordinates to a tenth of a millimetre',()=>{
  const map=normalize({format:'hotdog-map',version:1,bounds:[0,0,2.00004,1],starts:[{p:[0.5,0.5]}],spawns:[{p:[1.5,0.5]}],blocks:[{rect:[0.90001,0.4,1.1,0.6]}],ramps:[{rect:[0.1,0.1,0.4,0.2],height:0.1,rise:'-z'}],props:[{kind:'box',rect:[1.2,0.1,1.3,0.2],height:0.05}]});
  assert.equal(map.name,'Untitled map');
  assert.deepEqual(map.bounds,[0,0,2,1]);
  assert.deepEqual(map.nav,{cell:.04,clearance:.035});
  assert.deepEqual(map.starts,[{p:[.5,.5],yaw:0}]);
  assert.deepEqual(map.blocks,[{rect:[.9,.4,1.1,.6],height:.25,style:'steel'}]);
  assert.deepEqual(map.ramps,[{rect:[.1,.1,.4,.2],base:0,height:.1,rise:'-z'}]);
  assert.deepEqual(map.props,[{kind:'box',rect:[1.2,.1,1.3,.2],base:0,height:.05,style:'wood'}]);
  assert.deepEqual([map.pickups,map.decks],[[],[]]);
});

test('normalising accepts a JSON string and survives a round trip',()=>{
  const map=normalize(text);
  assert.deepEqual(normalize(stringify(map)),map);
});

for(const [name,change,pattern] of [
  ['a foreign file',m=>{m.format='tiled'},/format/],
  ['a newer format version',m=>{m.version=3},/version/],
  ['a version between',m=>{m.version=1.5},/version/],
  ['non-numeric coordinates',m=>{m.blocks[0].rect[0]='left'},/blocks\[0\]/],
  ['an inside-out rectangle',m=>{m.blocks[2].rect=[.5,2.7,.2,2.9]},/blocks\[2\]/],
  ['a map without player starts',m=>{m.starts=[]},/start/],
  ['a map without zombie spawns',m=>{m.spawns=[]},/spawn/],
  ['an unknown material',m=>{m.blocks[2].style='lava'},/style/],
  ['an appliance kit on a deck',m=>{m.decks[0].style='microwave'},/style/],
  ['a weapon that does not exist',m=>{m.pickups[0].weapon=8},/weapon/],
  ['a ramp without a direction',m=>{delete m.ramps[0].rise},/rise/],
  ['a ramp that does not rise',m=>{m.ramps[0].base=.28},/ramps\[0\]/],
  ['a navigation grid too large to search',m=>{m.bounds=[-50,-50,50,50];m.nav.cell=.01},/grid/],
]) test(`normalising rejects ${name}`,()=>{assert.throws(()=>normalize(edit(change)),error=>error instanceof MapError&&pattern.test(error.message))});

test('normalising rejects input that is not a map object',()=>{
  for(const input of [null,42,[],'{"broken"','[]'])assert.throws(()=>normalize(input),MapError);
});

test('the default grid is ninety by thirty-four cells and blocks what the hard-coded rules blocked',()=>{
  const l=level();
  assert.deepEqual([l.cols,l.rows],[90,34]);
  let blocked=0;
  for(let id=0;id<l.cols*l.rows;id++){const [x,z]=l.cellCenter(id);assert.equal(!!l.blocked[id],!legacyClear(x,z),`cell ${id} at ${x},${z}`);blocked+=l.blocked[id]}
  assert.ok(blocked>150&&blocked<600,`${blocked} blocked cells`);
});

test('grid dimensions use exact arithmetic instead of float rounding',()=>{
  // 1.36 / 0.04 is 34.000004 in single precision; a float ceil would add a 35th row off the counter.
  assert.equal(compile(normalize(edit(m=>{m.bounds=[-1.8,2.27,1.8,3.63]}))).rows,34);
  assert.equal(compile(normalize(edit(m=>{m.bounds=[-1.8,2.27,1.8,3.64]}))).rows,35);
  const odd=compile(normalize(edit(m=>{m.nav.cell=.033})));
  assert.deepEqual([odd.cols,odd.rows],[110,42]);
});

test('a block edge that lands exactly on a cell centre blocks that cell',()=>{
  // Cell centres sit at -1.78 + 0.04 i. The block edge plus clearance lands exactly on the centre at x = -0.50.
  const l=compile(normalize(edit(m=>{m.blocks=[{rect:[-.465,2.6,-.3,2.8]}]})));
  const centre=l.cellIndex(-.50,2.69);
  assert.deepEqual(l.cellCenter(centre).map(v=>+v.toFixed(6)),[-.5,2.69]);
  assert.equal(l.blocked[centre],1);
  assert.equal(l.blocked[centre-1],0);
});

test('clear() agrees with the grid and rejects positions off the map',()=>{
  const l=level();
  assert.equal(l.clear(.83,3.23),true);
  assert.equal(l.clear(-.34,2.81),false);
  for(const [x,z] of [[-1.81,3],[1.8,3],[0,2.26],[0,3.63],[NaN,3]])assert.equal(l.clear(x,z),false,`${x},${z}`);
});

test('ground height matches the hard-coded deck and ramps',()=>{
  const l=level(),next=random(7);
  for(let i=0;i<20000;i++){const x=-1.9+next()*3.8,z=2.2+next()*1.5;assert.ok(Math.abs(l.ground(x,z)-legacyGround(x,z))<1e-9,`${x},${z}`)}
  for(const [x,z] of [[1.25,3.5],[1.6,3.16],[.55,3.5],[1.65,2.46],[1.76,3.61],[0,3]])assert.ok(Math.abs(l.ground(x,z)-legacyGround(x,z))<1e-9,`${x},${z}`);
  assert.equal(l.height(1.5,3.4),.28);
});

test('ramps rise along any of the four directions from their base',()=>{
  const l=compile(normalize(edit(m=>{m.decks=[];m.ramps=[
    {rect:[-1,2.4,-.6,2.6],base:.1,height:.3,rise:'-x'},{rect:[-.2,2.4,0,2.9],height:.2,rise:'-z'}]})));
  assert.ok(Math.abs(l.height(-1,2.5)-.3)<1e-12);
  assert.ok(Math.abs(l.height(-.6,2.5)-.1)<1e-12);
  assert.ok(Math.abs(l.height(-.8,2.5)-.2)<1e-12);
  assert.ok(Math.abs(l.height(-.1,2.4)-.2)<1e-12);
  assert.ok(Math.abs(l.height(-.1,2.9))<1e-12);
  assert.equal(l.height(.5,2.5),0);
});

test('where surfaces overlap the highest one is the ground',()=>{
  const l=compile(normalize(edit(m=>{m.ramps=[];m.decks=[{rect:[-1,2.4,0,3],height:.1},{rect:[-.5,2.4,.5,3],height:.3}]})));
  assert.equal(l.height(-.75,2.7),.1);
  assert.equal(l.height(-.25,2.7),.3);
  assert.equal(l.height(.25,2.7),.3);
});

test('stepping rules match the hard-coded ones on the floor, the ramps and the deck',()=>{
  const l=level(),next=random(11);let allowed=0;
  for(let i=0;i<40000;i++){
    const x=-1.85+next()*3.7,z=2.22+next()*1.46;
    const from=[x,legacyGround(x,z)+(next()<.2?next()*.12:0),z];
    const to=[x+(next()-.5)*.08,from[1],z+(next()-.5)*.08];
    const expected=legacyCanStep(from,to);allowed+=expected;
    assert.equal(l.canStep(from,to),expected,`${from} -> ${to}`);
  }
  assert.ok(allowed>10000&&allowed<39000,`${allowed} of 40000 steps allowed`);
});

test('both ramps reach the deck on foot but its sides cannot be climbed',()=>{
  const l=level();
  for(const [[ax,az],[bx,bz]] of [[[.55,3.53],[1.40,3.53]],[[1.65,2.46],[1.65,3.30]]]){
    let p=[ax,FLOOR,az];
    for(let i=1;i<=100;i++){const q=[ax+(bx-ax)*i/100,p[1],az+(bz-az)*i/100];assert.ok(l.canStep(p,q),`${p} -> ${q}`);p=[q[0],l.ground(q[0],q[2]),q[2]]}
    assert.ok(Math.abs(p[1]-FLOOR-.28)<1e-9);
  }
  assert.equal(l.canStep([1.2,FLOOR,3.3],[1.26,FLOOR,3.3]),false);
});

test('a jump carries a hotdog over a block only when it clears the top',()=>{
  const l=compile(normalize(edit(m=>{m.blocks[2].height=.1})));
  assert.equal(l.walkable(-.34,FLOOR,2.81),false);
  assert.equal(l.walkable(-.34,FLOOR+.1,2.81),false);
  assert.equal(l.walkable(-.34,FLOOR+.106,2.81),true);
});

test('shots are stopped by blocks exactly as the hard-coded cover test did',()=>{
  const l=level(),next=random(23);let hits=0;
  for(let i=0;i<20000;i++){
    const o=[-1.75+next()*3.5,FLOOR+next()*.4,2.3+next()*1.3];
    const yaw=next()*Math.PI*2,pitch=(next()-.5)*.9,d=[Math.cos(pitch)*Math.sin(yaw),Math.sin(pitch),Math.cos(pitch)*Math.cos(yaw)];
    const expected=legacyCover(o,d);hits+=expected<12;
    assert.ok(Math.abs(l.cover(o,d)-expected)<1e-9,`${o} along ${d}`);
  }
  assert.ok(hits>2000,`${hits} of 20000 rays hit cover`);
});

test('cover follows each block\'s own height',()=>{
  const l=compile(normalize(edit(m=>{m.blocks[2].height=.1})));
  const overTheTop=l.cover([-.34,FLOOR+.135,2.5],[0,0,1]);
  const intoTheSide=l.cover([-.34,FLOOR+.05,2.5],[0,0,1]);
  assert.equal(overTheTop,12);
  assert.ok(Math.abs(intoTheSide-.2)<1e-9);
});

test('routes go around cover, stay on clear cells and refuse blocked goals',()=>{
  const l=level();
  const route=l.path([-.7,2.81],[.1,2.81]);
  assert.ok(route.length>1);
  assert.ok(route.every(p=>l.clear(...p)));
  assert.deepEqual(route.at(-1),[.1,2.81]);
  let previous=[-.7,2.81];for(const p of route){assert.ok(l.line(previous,p));previous=p}
  assert.deepEqual(l.path([-.7,2.81],[-.34,2.81]),[]);
  assert.deepEqual(l.path([.83,3.23],[1,3.2]),[[1,3.2]]);
});

test('routes report walled-off goals as unreachable and never cut corners',()=>{
  const l=compile(normalize(edit(m=>{m.decks=[];m.ramps=[];m.blocks=[{rect:[-.02,2.27,.02,3.63],height:.25}]})));
  assert.deepEqual(l.path([-1,3],[1,3]),[]);
  const gap=compile(normalize(edit(m=>{m.decks=[];m.ramps=[];m.blocks=[{rect:[-.02,2.27,.02,2.9],height:.25},{rect:[-.02,3.1,.02,3.63],height:.25}]})));
  const through=gap.path([-1,2.5],[1,2.5]);
  assert.ok(through.length>=2);
  assert.ok(through.some(p=>p[1]>2.93&&p[1]<3.07));
});

test('routes cannot squeeze diagonally between two blocks that touch at a corner',()=>{
  // Two quadrants are solid, so the open ones meet only at a single corner point.
  const corner=compile(normalize(edit(m=>{m.nav.clearance=0;m.decks=[];m.ramps=[];m.blocks=[{rect:[-1.8,2.27,0,2.95]},{rect:[0,2.95,1.8,3.63]}]})));
  assert.equal(corner.clear(-1,3.3),true);
  assert.equal(corner.clear(1,2.5),true);
  assert.deepEqual(corner.path([-1,3.3],[1,2.5]),[]);
});

test('the short way round is chosen even when it starts by leading away from the goal',()=>{
  // A corridor runs straight towards the goal but ends in three baffles; leaving by the back door and
  // walking around the outside is about 3.4 m against 4.8 m. A search that trusts its estimate too much takes the corridor.
  const c=.04,cells=(i0,j0,i1,j1)=>({rect:[i0*c+.01,j0*c+.01,(i1+1)*c-.01,(j1+1)*c-.01]});
  const maze=compile(normalize({format:'hotdog-map',version:1,bounds:[0,0,2.4,1.6],nav:{cell:c,clearance:0},starts:[{p:[.42,.82]}],spawns:[{p:[2.02,.82]}],blocks:[
    cells(5,36,47,36),cells(5,4,47,4),cells(5,5,5,18),cells(5,22,5,35),cells(47,5,47,18),cells(47,22,47,35),
    cells(6,22,38,22),cells(6,18,38,18),cells(41,5,41,31),cells(43,9,43,35),cells(45,5,45,31)]}));
  const start=[.42,.82],goal=[2.02,.82],route=maze.path(start,goal);
  let length=0,previous=start;for(const p of route){length+=Math.hypot(p[0]-previous[0],p[1]-previous[1]);previous=p}
  assert.ok(route.some(p=>p[1]<.16),'the route passes outside, below the bottom wall');
  assert.ok(length>3&&length<3.7,`route is ${length.toFixed(2)} m`);
});

test('routes are as short as an exhaustive search of the grid allows',()=>{
  const l=compile(normalize(read('./fixtures/awkward.json'))),{cols,rows,blocked,cell}=l,next=random(31);
  // Plain Dijkstra over the same grid, without any heuristic to get wrong.
  const exhaustive=(from,to)=>{
    const cost=new Float64Array(cols*rows).fill(Infinity),open=[[0,from]];cost[from]=0;
    while(open.length){
      open.sort((a,b)=>b[0]-a[0]);const [c,id]=open.pop();if(id===to)return c;if(c>cost[id])continue;
      const x=id%cols,z=(id-x)/cols;
      for(let dz=-1;dz<=1;dz++)for(let dx=-1;dx<=1;dx++){
        const nx=x+dx,nz=z+dz;if((!dx&&!dz)||nx<0||nz<0||nx>=cols||nz>=rows||blocked[nz*cols+nx])continue;
        if(dx&&dz&&(blocked[z*cols+nx]||blocked[nz*cols+x]))continue;
        const n=nz*cols+nx,d=c+(dx&&dz?Math.SQRT2:1);if(d<cost[n]){cost[n]=d;open.push([d,n])}
      }
    }
    return Infinity;
  };
  let compared=0;
  while(compared<12){
    const a=[l.x0+next()*(l.x1-l.x0),l.z0+next()*(l.z1-l.z0)],b=[a[0]+(next()-.5)*1.6,a[1]+(next()-.5)*1.6];
    if(!l.clear(...a)||!l.clear(...b)||l.line(a,b))continue;
    const best=exhaustive(l.cellIndex(...a),l.cellIndex(...b)),route=l.path(a,b);
    if(best===Infinity){assert.deepEqual(route,[]);continue}
    let length=0,previous=a;for(const p of route){length+=Math.hypot(p[0]-previous[0],p[1]-previous[1]);previous=p}
    // Pulling the route taut can only shorten it; the slack covers the walk between the end points and their cell centres.
    assert.ok(length<=best*cell+1.5*cell,`route of ${length.toFixed(3)} m where ${(best*cell).toFixed(3)} m is possible, ${a} -> ${b}`);
    compared++;
  }
});

test('a repeated search gives the same route',()=>{
  const l=level();
  const first=l.path([-1.6,2.4],[1.1,3.5]);
  for(let i=0;i<5;i++){l.path([1.7,2.3],[-1.7,3.6]);assert.deepEqual(l.path([-1.6,2.4],[1.1,3.5]),first)}
});

test('a hotdog below a raised target is sent to the foot of the cheapest ramp, then up it',()=>{
  const l=level(),target=[1.5,FLOOR+.28,3.4];
  const near=(a,b)=>Math.hypot(a[0]-b[0],a[2]-b[2])<1e-9;
  assert.ok(near(l.climbGoal([.2,FLOOR,3],target),[.58,0,3.53]));
  assert.ok(near(l.climbGoal([1.7,FLOOR,2.3],target),[1.65,0,2.49]));
  assert.ok(near(l.climbGoal([.6,FLOOR,3.53],target),[1.4,0,3.53]));
  assert.ok(near(l.climbGoal([.9,FLOOR+.14,3.53],target),[1.4,0,3.53]));
  assert.ok(near(l.climbGoal([1.65,FLOOR+.1,2.71],target),[1.65,0,3.31]));
});

test('no climb is planned for a target on the same level',()=>{
  const l=level();
  assert.equal(l.climbGoal([.2,FLOOR,3],[-1,FLOOR,2.5]),null);
  assert.equal(l.climbGoal([1.4,FLOOR+.28,3.5],[1.6,FLOOR+.28,3.3]),null);
  assert.equal(compile(normalize(edit(m=>{m.ramps=[]}))).climbGoal([.2,FLOOR,3],[1.5,FLOOR+.28,3.4]),null);
});

test('spawn, start and pickup positions stand on the ground under them',()=>{
  const l=compile(normalize(edit(m=>{m.starts[1].p=[1.5,3.5];m.pickups[0].p=[1.4,3.55]})));
  assert.deepEqual(l.startPosition(0),[.83,FLOOR,3.23]);
  assert.deepEqual(l.startPosition(1),[1.5,FLOOR+.28,3.5]);
  assert.deepEqual(l.startPosition(5),[.83,FLOOR,3.23]);
  assert.deepEqual(l.pickupPosition(0),[1.4,FLOOR+.28+.04,3.55]);
  assert.deepEqual(l.pickupPosition(1),[.06,FLOOR+.04,3.4]);
  // The wave used x = -1.23 + (n % 3) * 0.035 for the n-th zombie, counting from one.
  assert.deepEqual([1,2,3,4].map(n=>l.spawnPosition(n)[0]),[-1.195,-1.16,-1.23,-1.195]);
  assert.deepEqual(l.spawnPosition(1).slice(1),[FLOOR,3.02]);
});

test('the bundled map has no problems: everything is reachable on foot and by zombies',()=>{
  const l=level(),a=analyze(l);
  assert.deepEqual(a.issues,[]);
  for(const s of l.map.starts){const id=l.cellIndex(...s.p);assert.equal(a.reach[id],1);assert.ok(a.zdist[id]>0)}
  for(const p of l.map.pickups)assert.equal(a.reach[l.cellIndex(...p.p)],1);
  const deck=l.cellIndex(1.5,3.5);
  assert.equal(a.reach[deck],1);
  assert.ok(Math.abs(a.groundH[deck]-.28)<1e-6);
  assert.equal(a.stand[l.cellIndex(-.34,2.81)],0);
  // (Eight guns, and the two boxes of ammunition every siege map has now that rounds run out in every mode.)
  assert.deepEqual([a.stats.cells,a.stats.blocks,a.stats.pickups],[3060,4,10]);
});

test('zombie travel distance is measured in metres from the nearest spawn',()=>{
  const l=compile(normalize(edit(m=>{m.blocks=[];m.decks=[];m.ramps=[];m.spawns=[{p:[-1.5,2.45]}]}))),a=analyze(l);
  const spawn=l.cellIndex(-1.5,2.45);
  assert.equal(a.zdist[spawn],0);
  assert.ok(Math.abs(a.zdist[spawn+10]-.4)<1e-6,`ten cells east is ${a.zdist[spawn+10]} m`);
  assert.ok(Math.abs(a.zdist[spawn+10*l.cols+10]-.56)<1e-6,`ten cells diagonally is ${a.zdist[spawn+10*l.cols+10]} m`);
  const walled=analyze(compile(normalize(edit(m=>{m.blocks.push({rect:[-.9,2.27,-.85,3.63]})}))));
  assert.equal(walled.zdist[l.cellIndex(.83,3.23)],-1);
});

const codes=map=>analyze(compile(normalize(map))).issues.map(issue=>`${issue.severity}:${issue.code}:${issue.kind}[${issue.index}]`);
test('analysis reports a start inside a block as an error',()=>{
  assert.deepEqual(codes(edit(m=>{m.starts[2].p=[-.34,2.81]})),['error:start-blocked:starts[2]']);
});
test('analysis reports a start off the map as an error',()=>{
  assert.deepEqual(codes(edit(m=>{m.starts[0].p=[2.5,3]})),['error:start-blocked:starts[0]']);
});
test('analysis reports a zombie spawn that cannot move as an error',()=>{
  assert.deepEqual(codes(edit(m=>{m.spawns[1].p=[-1.2,3.2]})),['error:spawn-blocked:spawns[1]']);
});
test('analysis warns about a pickup nobody can walk to',()=>{
  const walled=edit(m=>{m.blocks.push({rect:[-1.75,2.3,-1.7,2.5]},{rect:[-1.6,2.3,-1.55,2.5]},{rect:[-1.75,2.5,-1.55,2.55]},{rect:[-1.8,2.27,-1.55,2.3]})});
  assert.deepEqual(codes(walled),['warning:pickup-unreachable:pickups[3]']);
});
test('analysis warns about a deck without a way up',()=>{
  assert.deepEqual(codes(edit(m=>{m.ramps=[]})),['warning:deck-unreachable:decks[0]']);
});
test('analysis warns when zombies cannot reach any hotdog',()=>{
  const sealed=edit(m=>{m.blocks.push({rect:[-.9,2.27,-.85,3.63]})});
  assert.ok(codes(sealed).includes('warning:spawn-isolated:spawns[0]'));
});
test('analysis warns about a ramp steeper than its run',()=>{
  assert.ok(codes(edit(m=>{m.ramps[0].rect=[1.05,3.45,1.25,3.61]})).includes('warning:ramp-steep:ramps[0]'));
});
test('analysis warns when five hotdogs must share fewer starts',()=>{
  assert.deepEqual(codes(edit(m=>{m.starts.length=3})),['warning:few-starts:starts[0]']);
});
test('analysis warns about geometry lying entirely off the map',()=>{
  assert.deepEqual(codes(edit(m=>{m.blocks.push({rect:[3,3,3.2,3.2]})})),['warning:outside:blocks[4]']);
});
test('analysis issues carry a readable message',()=>{
  const [issue]=analyze(compile(normalize(edit(m=>{m.starts[2].p=[-.34,2.81]})))).issues;
  assert.match(issue.message,/start 3/i);
});

test('compiling and analysing a large crowded map stays fast',()=>{
  const next=random(5),map=edit(m=>{m.bounds=[-10,-10,10,10];m.nav.cell=.04;m.starts=[{p:[0,0]}];m.spawns=[{p:[9,9]}];m.pickups=[];m.decks=[];m.ramps=[];m.props=[];m.blocks=[];
    for(let i=0;i<2000;i++){const x=-9.8+next()*19.2,z=-9.8+next()*19.2;if(Math.hypot(x,z)<.6||Math.hypot(x-9,z-9)<.6)continue;m.blocks.push({rect:[x,z,x+.05+next()*.3,z+.05+next()*.3],height:.25})}
    for(let i=0;i<300;i++){const x=-9.5+next()*18,z=-9.5+next()*18;m.decks.push({rect:[x,z,x+.5,z+.5],height:.3});m.ramps.push({rect:[x-.6,z,x,z+.5],height:.3,rise:'+x'})}});
  const normal=normalize(map);
  const started=performance.now();
  const l=compile(normal),a=analyze(l);
  const elapsed=performance.now()-started;
  assert.equal(l.cols*l.rows,250000);
  assert.ok(a.stats.reachable>50000,`${a.stats.reachable} reachable cells`);
  assert.ok(elapsed<400,`compile + analyse took ${elapsed.toFixed(1)} ms`);
  const search=performance.now();
  for(let i=0;i<20;i++)l.path([0,0],[9,9]);
  assert.ok((performance.now()-search)/20<40,`a long route took ${((performance.now()-search)/20).toFixed(2)} ms`);
});

// ---- Forts: the mode, the teams, the zones (bases, forcefield, bacon) and what the analysis asks of such a map ----
// These tests work on a Forts map kept for them, fixtures/long-fort.json: six metres of counter, the layout of the bundled Forts map. The kitchen grows to
// hold it, so nothing of its analysis is left aside.
const fortText=read('./fixtures/long-fort.json');
const analyzeFort=analyze;
const fort=()=>JSON.parse(fortText);
const fortEdit=change=>{const map=fort();change(map);return map};
const tiny=(more={})=>({format:'hotdog-map',version:1,bounds:[0,0,4,2],starts:[{p:[.5,1]}],spawns:[{p:[2,.3]}],...more});

test('the Forts map is stored in canonical form and has no problems',()=>{
  const map=normalize(fortText);
  assert.deepEqual(map,{...fort(),sinks:[]});
  assert.equal(stringify(map),fortText);
  assert.deepEqual(Object.keys(map),['format','version','name','theme','mode','build','bounds','nav',...KINDS]);
  assert.equal(KINDS.at(-1),'zones');
  const a=analyzeFort(compile(map));
  assert.deepEqual(a.issues,[]);
  assert.deepEqual([a.stats.starts,a.stats.props,a.stats.zones],[6,30,4]);
});

test('a map says its mode: a siege unless it says forts or kungfu, and only a Forts map has a build time',()=>{
  assert.deepEqual([MODES,TEAMS,ZONES],[['siege','forts','kungfu'],['mustard','ketchup'],['base','field','bacon']]);
  const dojo=normalize(tiny({mode:'kungfu',build:60}));
  assert.deepEqual([dojo.mode,'build' in dojo,compile(dojo).fort],['kungfu',false,null]);
  const siege=normalize(tiny({build:60}));
  assert.equal(siege.mode,'siege');
  assert.equal('build' in siege,false);
  assert.deepEqual(siege.zones,[]);
  assert.deepEqual(Object.keys(normalize(text)).slice(0,7),['format','version','name','theme','mode','bounds','nav']);
  assert.equal(normalize(tiny({mode:'forts'})).build,BUILD);
  assert.equal(normalize(tiny({mode:'forts',build:42.00004})).build,42);
  assert.equal(compile(siege).fort,null);
  assert.equal(fortress(siege),null);
});

test('zones and team starts are checked, put in canonical form and survive a round trip',()=>{
  const map=normalize(tiny({mode:'forts',starts:[{p:[.5,1],team:'mustard',extra:1},{p:[3.5,1],yaw:1,team:'ketchup'},{p:[1,1],team:null}],
    zones:[{kind:'bacon',p:[2.00004,1],rect:[0,0,1,1]},{kind:'field',rect:[1.95,0,2.05,2],team:'mustard'},{kind:'base',team:'ketchup',rect:[3.2,.5,4,1.5],style:'red'}]}));
  assert.deepEqual(map.starts,[{p:[.5,1],yaw:0,team:'mustard'},{p:[3.5,1],yaw:1,team:'ketchup'},{p:[1,1],yaw:0}]);
  assert.deepEqual(map.zones,[{kind:'bacon',p:[2,1]},{kind:'field',rect:[1.95,0,2.05,2]},{kind:'base',team:'ketchup',rect:[3.2,.5,4,1.5]}]);
  assert.deepEqual(normalize(stringify(map)),map);
  assert.match(stringify(map),/\{"kind": "base", "team": "ketchup", "rect": \[3\.2, 0\.5, 4, 1\.5\]\}/);
  // A siege map's starts may name teams too; nothing else changes.
  assert.deepEqual(normalize(tiny({starts:[{p:[.5,1],team:'ketchup'}]})).starts,[{p:[.5,1],yaw:0,team:'ketchup'}]);
  assert.deepEqual(normalizeItem('zones',{kind:'bacon',p:[1,1]}),{kind:'bacon',p:[1,1]});
});

for(const [name,more,pattern] of [
  ['a mode that does not exist',{mode:'ctf'},/mode/],
  ['a build time out of range',{mode:'forts',build:2},/build/],
  ['a start on a team that does not exist',{starts:[{p:[.5,1],team:'mayo'}]},/starts\[0\]\.team/],
  ['a zone of an unknown kind',{zones:[{kind:'flag',p:[1,1]}]},/zones\[0\]\.kind/],
  ['a zone without a kind',{zones:[{rect:[0,0,1,1]}]},/zones\[0\]\.kind/],
  ['a base without a team',{zones:[{kind:'base',rect:[0,0,1,1]}]},/zones\[0\]\.team/],
  ['a base on a team that does not exist',{zones:[{kind:'base',team:'relish',rect:[0,0,1,1]}]},/zones\[0\]\.team/],
  ['an inside-out forcefield',{zones:[{kind:'field',rect:[2,0,1.9,2]}]},/zones\[0\]\.rect/],
  ['bacon without a spot',{zones:[{kind:'bacon'}]},/zones\[0\]\.p/],
  ['zones that are not a list',{zones:{}},/zones/],
  ['more zones than a map may hold',{zones:Array.from({length:LIMITS.zones+1},()=>({kind:'bacon',p:[1,1]}))},/limit/],
])test(`normalising rejects ${name}`,()=>{
  assert.throws(()=>normalize(tiny(more)),error=>error instanceof MapError&&pattern.test(error.message));
});

test('a Forts map is read into its field, bases, sides, bacon and team starts',()=>{
  const l=compile(normalize(fortText)),f=l.fort;
  assert.deepEqual([f.axis,f.mid,f.field,f.sides,f.bacon,f.build],[0,0,[-.04,2.2,.04,4],[-1,1],[0,3.1],150]);
  assert.deepEqual(f.bases,[[-3,2.7,-2.5,3.5],[2.5,2.7,3,3.5]]);
  assert.deepEqual(f.starts.map(own=>own.map(s=>s.p[0])),[[-2.75,-2.75,-2.75],[2.75,2.75,2.75]]);
  assert.deepEqual(l.teamStart(0,1),{p:[-2.75,FLOOR,3.1],yaw:-1.5708});
  assert.deepEqual(l.teamStart(1,3),{p:[2.75,FLOOR,2.9],yaw:1.5708});
  // The bacon lies on the deck in the middle.
  assert.deepEqual(l.baconPosition(),[0,FLOOR+.12,3.1]);
  assert.deepEqual([l.inBase(0,-2.6,3),l.inBase(1,-2.6,3),l.inBase(1,2.6,3),l.inBase(1,2.6,3.6)],[true,false,true,false]);
  assert.deepEqual([l.side(-.01,3),l.side(.01,3),l.side(-2,2.3)],[-1,1,-1]);
});

test('what a Forts map leaves out is assumed: a field across the middle, bases at the ends, the bacon between, starts by side',()=>{
  const f=fortress(normalize(edit(m=>{m.mode='forts'})));
  assert.deepEqual([f.axis,f.mid,f.sides,f.build],[0,0,[-1,1],BUILD]);
  assert.deepEqual(f.field,[-.03,2.27,.03,3.63]);
  assert.deepEqual(f.bases,[[-1.8,2.27,-1.35,3.63],[1.35,2.27,1.8,3.63]]);
  assert.ok(Math.abs(f.bacon[0])<1e-12&&Math.abs(f.bacon[1]-2.95)<1e-12);
  assert.deepEqual(f.starts.map(own=>own.map(s=>s.p)),[[[-.05,2.52],[-.65,3.55]],[[.83,3.23],[.3,3.08],[.94,2.48]]]);
  // A deep map is divided along z; a wide field says so itself. A team with no start begins in the middle of its base, facing the field.
  const deep=fortress(normalize(tiny({mode:'forts',bounds:[0,0,2,4],starts:[{p:[1,.5]}]})));
  assert.deepEqual([deep.axis,deep.mid,deep.field,deep.bases],[1,2,[0,1.97,2,2.03],[[0,0,2,.5],[0,3.5,2,4]]]);
  assert.deepEqual(deep.starts,[[{p:[1,.5],yaw:0}],[{p:[1,3.75],yaw:0}]]);
  const across=fortress(normalize(tiny({mode:'forts',zones:[{kind:'field',rect:[0,.9,4,1]},{kind:'base',team:'mustard',rect:[1,1.5,3,2]}],starts:[{p:[.5,.3],team:'ketchup'}]})));
  assert.deepEqual([across.axis,across.sides,across.bases[1]],[1,[1,-1],[0,0,4,.25]]);
  assert.deepEqual(across.starts,[[{p:[2,1.75],yaw:0}],[{p:[.5,.3],yaw:0}]]);
  const wide=fortress(normalize(tiny({mode:'forts',starts:[{p:[3.5,1]}]})));
  assert.deepEqual(wide.starts[0],[{p:[.25,1],yaw:-Math.PI/2}]);
});

test('while the forcefield is up it stops routes, steps and shots, and the map itself is unchanged',()=>{
  const map=normalize(fortText),open=compile(map),shut=fielded(map);
  assert.equal(shut.map,map);
  assert.equal(map.blocks.length,5);
  assert.deepEqual(shut.fort,open.fort);
  assert.ok(open.path([-1,2.6],[1,2.6]).length>0);
  assert.deepEqual(shut.path([-1,2.6],[1,2.6]),[]);
  assert.ok(shut.path([-1,2.6],[-2.7,3.4]).length>0);
  assert.equal(open.cover([-.5,FLOOR+.1,2.6],[1,0,0]),12);
  assert.ok(Math.abs(shut.cover([-.5,FLOOR+.1,2.6],[1,0,0])-.46)<1e-9);
  // Nothing gets over it, even from the deck in the middle.
  assert.ok(Math.abs(shut.cover([-.2,FLOOR+2.5,3.1],[1,0,0])-.16)<1e-9);
  assert.equal(open.canStep([-.08,FLOOR,2.6],[-.06,FLOOR,2.6]),true);
  assert.equal(shut.canStep([-.08,FLOOR,2.6],[-.06,FLOOR,2.6]),false);
  assert.equal(shut.walkable(0,FLOOR+2,2.6),false);
  assert.equal(shut.clear(0,2.6),false);
  assert.equal(open.clear(0,2.6),true);
  // A map that names no field is closed along the one that is assumed.
  const bare=fielded(normalize(edit(m=>{m.mode='forts'})));
  assert.deepEqual(bare.path([-.5,2.4],[.9,2.4]),[]);
});

for(const [name,change,expected,message] of [
  ['a missing forcefield',m=>{m.zones.splice(2,1)},['error:field-missing:zones[-1]'],/forcefield/],
  ['missing bacon',m=>{m.zones.splice(3,1)},['error:bacon-missing:zones[-1]'],/bacon/],
  ['a team without a base',m=>{m.zones.splice(0,1)},['error:base-missing:zones[-1]'],/Mustard Eaters have no base/],
  ['a second forcefield, bacon or base',m=>{m.zones.push({kind:'field',rect:[1,2.2,1.08,4]},{kind:'bacon',p:[.5,3.1]},{kind:'base',team:'mustard',rect:[-3,2.2,-2.5,2.6]})},['warning:zone-extra:zones[4]','warning:zone-extra:zones[5]','warning:zone-extra:zones[6]'],/second/],
  ['both bases on one side of the field',m=>{m.zones[1].rect=[-2.4,3.6,-2.2,3.9];m.starts.splice(3,3,{p:[-2.3,3.75],team:'ketchup'},{p:[-2.3,3.8],team:'ketchup'},{p:[-2.3,3.7],team:'ketchup'})},['error:bases-same-side:zones[1]'],/same side/],
  ['a base lying across the field',m=>{m.zones[0].rect=[-.5,2.7,.1,3.5]},['error:base-in-field:zones[0]'],/across the forcefield/],
  ['a forcefield hotdogs can walk round',m=>{m.zones[2].rect=[-.04,2.2,.04,3]},['error:field-open:zones[2]'],/does not close/],
  ['a start that belongs to no team',m=>{delete m.starts[0].team},['error:start-teamless:starts[0]','warning:team-few-starts:starts[-1]'],/Start 1 belongs to no team/],
  ['a start on the wrong side of the field',m=>{m.starts[0].p=[2.6,3]},['error:start-wrong-side:starts[0]'],/Start 1 belongs to the Mustard Eaters/],
  ['a team without a start',m=>{m.starts.splice(0,3)},['error:team-no-start:starts[-1]','warning:few-starts:starts[0]'],/Mustard Eaters have no start/],
  ['a team with fewer than three starts',m=>{m.starts.splice(4,1)},['warning:team-few-starts:starts[-1]'],/Ketchup Connoisseurs have fewer than 3 starts/],
  ['bacon inside a block',m=>{m.zones[3].p=[0,3.85]},['error:bacon-blocked:zones[3]'],/bacon/],
  ['bacon a team cannot walk to',m=>{m.blocks.push({rect:[.9,2.2,1,4]})},['error:bacon-unreachable:zones[3]'],/Ketchup Connoisseurs cannot reach the bacon/],
  ['a base the bacon cannot be carried up to',m=>{m.decks.push({rect:[-3,2.6,-2.4,3.6],height:.2})},['error:base-unreachable:zones[0]'],/Mustard Eaters cannot carry the bacon home/],
  ['a side with nothing to build with',m=>{m.props.splice(15,15)},['warning:stock-missing:props[-1]'],/Ketchup Connoisseurs have no loose objects/],
])test(`analysis of a Forts map reports ${name}`,()=>{
  const map=fortEdit(change),issues=analyzeFort(compile(normalize(map))).issues;
  assert.deepEqual(issues.map(issue=>`${issue.severity}:${issue.code}:${issue.kind}[${issue.index}]`),expected);
  assert.match(issues[0].message,message);
});
test('analysis warns about part of the Forts layout on a map that cannot be played as Forts, and asks every gun map for five starts',()=>{
  assert.deepEqual(codes(edit(m=>{m.zones=[{kind:'bacon',p:[0,3]}]})),['warning:zones-partial:zones[0]']);
  assert.deepEqual(codes(edit(m=>{m.starts.length=3;m.starts[0].team='mustard'})),['warning:few-starts:starts[0]']);
  // A Forts map is asked for three starts a side instead.
  assert.equal(codes(fort()).length,0);
  assert.equal(TEAM_NAMES.length,2);
});
test('the mode is the game\'s: a map with the whole Forts layout can be played as Forts whatever it was made for, one with part of it cannot, and the dojo only as Kung Fu',()=>{
  // The long fort, made for a siege: still a Forts layout (its starts, naming their teams, as they are), so Forts can be played on it, and its checks run.
  const laid=normalize(fortEdit(m=>{m.mode='siege'}));
  assert.ok(fortLayout(laid)&&fortress(laid));assert.deepEqual(modes(laid),['siege','forts','ffa','cards']);
  assert.equal(codes(fortEdit(m=>{m.mode='siege'})).length,0);
  // Its starts naming no team: sorted by the side of the field they are on, as the game does.
  assert.equal(codes(fortEdit(m=>{m.mode='siege';for(const s of m.starts)delete s.team})).filter(c=>c.startsWith('error')).length,0);
  // Without the bacon: no Forts.
  const part=normalize(fortEdit(m=>{m.mode='siege';m.zones.splice(3,1)}));
  assert.ok(!fortLayout(part)&&!fortress(part));assert.deepEqual(modes(part),['siege','ffa','cards']);
  assert.deepEqual(codes(fortEdit(m=>{m.mode='siege';m.zones.splice(3,1)})),['warning:zones-partial:zones[0]']);
  // The bundled maps: Forts on the four Forts maps, the siege and Free For All on all but the dojo, which is Kung Fu's alone.
  const all=readdirSync(new URL('../../assets/maps/',import.meta.url)).filter(f=>f.endsWith('.json')).map(f=>[f,normalize(read(`../../assets/maps/${f}`))]);
  assert.deepEqual(all.filter(([,m])=>modes(m).includes('forts')).map(([f])=>f).sort(),['jungle-fort.json','moonlit-fort.json','pillow-fort.json','the-whole-kitchen-forts.json']);
  assert.equal(all.filter(([,m])=>modes(m).includes('siege')&&modes(m).includes('ffa')).length,all.length-1);
  assert.deepEqual(modes(all.find(([f])=>f==='kung-fu-dojo.json')[1]),['kungfu']);
});
test('a Forts map with nothing in it yet lists what it needs',()=>{
  const got=codes(edit(m=>{m.mode='forts';m.props=[]}));
  assert.deepEqual(got.filter(code=>code.startsWith('error')),['error:team-no-start:starts[-1]','error:team-no-start:starts[-1]',
    'error:start-teamless:starts[0]','error:start-teamless:starts[1]','error:start-teamless:starts[2]','error:start-teamless:starts[3]','error:start-teamless:starts[4]','error:field-missing:zones[-1]','error:bacon-missing:zones[-1]','error:base-missing:zones[-1]','error:base-missing:zones[-1]']);
  assert.deepEqual(got.filter(code=>code.includes('stock')),['warning:stock-missing:props[-1]','warning:stock-missing:props[-1]']);
});

// ---- The map the game ships for Forts, and the kitchen it has to fit in ----
test('the bundled Forts map is stored in canonical form, has no problems and fits inside the kitchen',()=>{
  const text=read('../../assets/maps/pillow-fort.json'),map=normalize(text);
  assert.equal(stringify(map),text);assert.deepEqual([map.name,map.mode,map.build],['Countertop Fort','forts',150]);
  const level=compile(map),a=analyze(level);
  assert.deepEqual(a.issues,[]);
  // A ketchup sniper, a hot sauce thrower, a relish bazooka, two lots of mustard, a syrup rifle (on the back deck) and a box of ammunition (at the base's
  // front corner) a side. The build stock is the kitchen's, at its true sizes, set out tidily: three cutting boards in a pile, a bread bin with two tins on
  // it, two napkins folded flat, four cereal boxes in a row, two more tins, five plates in a pile with a bowl on top, two rolls of paper towels, a bowl,
  // three mugs, two salt shakers and a pepper shaker a side: 58 loose objects in all, none of them in a base.
  assert.deepEqual([a.stats.starts,a.stats.spawns,a.stats.pickups,a.stats.props,a.stats.zones],[6,4,14,58,4]);
  const kinds=kind=>map.props.filter(prop=>prop.kind===kind&&prop.p[0]<0).length;
  assert.deepEqual(['board','breadbox','tin','napkin','cereal','plate','dish','roll','cup','salt','pepper'].map(kinds),[3,1,4,2,4,5,2,2,3,2,1]);
  assert.ok(map.props.every(prop=>!['pillow','chair','crate','bowl'].includes(prop.kind)),'only what a kitchen counter has');
  for(const side of [-1,1])assert.deepEqual(map.pickups.filter(item=>Math.sign(item.p[0])===side).map(item=>item.weapon).sort(),[0,1,1,2,3,4,6]);
  assert.ok(level.x0>=ROOM[0]&&level.x1<=ROOM[1]&&level.z1<=ROOM[2]);
  // Six metres by 1.8, longer than the siege's counter, and the same on both sides: every object has its mirror image across the field.
  assert.deepEqual(map.bounds,[-3,2.2,3,4]);
  const mirrored=list=>list.every(item=>list.some(other=>item.p?Math.abs(other.p[0]+item.p[0])<1e-9&&other.p[1]===item.p[1]:Math.abs(other.rect[0]+item.rect[2])<1e-9&&Math.abs(other.rect[2]+item.rect[0])<1e-9&&other.rect[1]===item.rect[1]&&other.rect[3]===item.rect[3]));
  for(const kind of ['starts','spawns','pickups','blocks','decks','ramps','props'])assert.ok(mirrored(map[kind]),`${kind} are mirrored`);
  assert.deepEqual(level.fort.sides,[-1,1]);assert.deepEqual(level.fort.starts.map(own=>own.length),[3,3]);
  // It is the layout the Forts tests play on (fixtures/long-fort.json), under its own name, but for its loose objects (the tests' are the old stock of
  // chairs, crates, pillows and napkins).
  // (And for the electric griddle across its middle, which makes it version 2: the tests' layout has none, so that the Forts tests play on a counter with
  // nothing to burn on.)
  assert.deepEqual({...map,name:'Long Fort',props:[],version:1,blocks:map.blocks.filter(block=>block.style!=='griddle')},{...normalize(fortText),props:[]});
});
// ---- The electric griddle: a counter thing of version 2 that is switched on and off and heats (crates/kitchen-sim/src/griddle.rs) ----
test('a griddle is one of the counter things, a block with a front on the side of its dial, and a map with one is version 2',()=>{
  assert.equal(KITS.at(-2),'griddle');assert.ok(KITS.indexOf('griddle')>=FIRST_KITS);
  const map=edit(m=>{m.blocks.push({rect:[1,3,1.5,3.28],height:.06,style:'griddle'})});
  const norm=normalize(map);
  assert.equal(norm.version,2);
  assert.deepEqual(norm.blocks.at(-1),{rect:[1,3,1.5,3.28],height:.06,style:'griddle',facing:'-z'},'its dial is on the -z side unless the map says');
  assert.equal(normalize({...map,blocks:[...map.blocks,{rect:[-1.5,3,-1,3.28],height:.06,style:'griddle',facing:'+x'}]}).blocks.at(-1).facing,'+x');
  assert.equal(normalize(base()).version,1,'a map without one is as it was');
  // Canonical, and its own canonical text; no problem with it standing on the counter (a hotdog jumps onto the 6 cm plate; it is a block to walk round).
  const text=stringify(norm);
  assert.equal(stringify(normalize(text)),text);
  assert.match(text,/"style": "griddle"/);
  assert.deepEqual(analyze(compile(norm)).issues.filter(issue=>issue.severity==='error'),[]);
  const level=compile(norm);
  assert.equal(level.clear(1.25,3.14),false,'a block: nobody walks through the plate');
  assert.equal(level.clear(1.25,3.5),true);
});
test('each Forts map has one griddle across the middle by the front of the counter, 50 by 28 by 6 cm, clear of the bases, the bacon, the pads, the starts, the spawns and the rest',()=>{
  for(const name of ['pillow-fort','jungle-fort','moonlit-fort','the-whole-kitchen-forts']){
    const text=read(`../../assets/maps/${name}.json`),map=normalize(text),griddles=map.blocks.filter(block=>block.style==='griddle');
    assert.equal(stringify(map),text,`${name} is canonical`);assert.equal(map.version,2);
    assert.equal(griddles.length,1,name);
    const [block]=griddles,[x0,z0,x1,z1]=block.rect,fort=fortress(map);
    assert.deepEqual([+(x1-x0).toFixed(4),+(z1-z0).toFixed(4),block.height,block.facing],[.5,.28,.06,'+z'],`${name}: 50 x 28 x 6 cm, the dial towards the middle of the counter`);
    assert.equal(+(x0+x1).toFixed(6),0,`${name}: on the centre line`);
    assert.ok(z0-map.bounds[1]>.05&&z0-map.bounds[1]<.6,`${name}: near the front edge`);
    const inside=(r,by)=>r[0]<x1+by&&r[2]>x0-by&&r[1]<z1+by&&r[3]>z0-by,near=(p,by)=>p[0]>x0-by&&p[0]<x1+by&&p[1]>z0-by&&p[1]<z1+by;
    for(const base of fort.bases)assert.equal(inside(base,0),false,`${name}: not in a base`);
    assert.equal(near(fort.bacon,.1),false,`${name}: the bacon`);
    for(const kind of ['starts','spawns','pickups'])for(const item of map[kind])assert.equal(near(item.p,.15),false,`${name}: ${kind}`);
    // A pad is further from the dial than a hotdog's reach (0.25 m) and the reach to a pad (0.15 m) together.
    const dial=[(x0+x1)/2,z1];
    for(const pad of map.pickups)assert.ok(Math.hypot(pad.p[0]-dial[0],pad.p[1]-dial[1])>.4,`${name}: a hotdog cannot reach a pad and the dial together`);
    for(const other of map.blocks.filter(each=>each!==block))assert.equal(inside(other.rect,.04),false,`${name}: ${other.style}`);
    for(const kind of ['decks','ramps'])for(const item of map[kind])assert.equal(inside(item.rect,.04),false,`${name}: ${kind}`);
    for(const prop of map.props)if(prop.p)assert.equal(near(prop.p,.12),false,`${name}: a ${prop.kind} at ${prop.p}`);
    assert.deepEqual(analyze(compile(map)).issues,[],name);
  }
});
test('bamboo is the last style, and a deck of it is round: the ellipse inside its rect is its floor, and a map with it is version 2',()=>{
  assert.deepEqual([STYLES.at(-1),FIRST_STYLES,ROUND,KITS.at(-1)],['bamboo',9,'bamboo','gong']);
  const map=normalize(edit(m=>{m.decks=[{rect:[-1,2.4,-.2,3.2],height:.03,style:'bamboo'},{rect:[.2,2.5,1.2,3],height:.04,style:'bamboo'}];m.ramps=[]}));
  assert.equal(map.version,2);assert.equal(normalize(edit(m=>{m.decks[0].style='wood'})).version,1,'the old styles stay version 1');
  assert.equal(normalize(edit(m=>{m.blocks[2].style='bamboo'})).version,2,'so does a block of it');
  const l=compile(map),a=analyze(l);
  // A circle 0.8 m across: on it within 0.4 m of its middle, off it in the corners of its square.
  assert.equal(l.height(-.6,2.8),.03);assert.equal(l.height(-.6+.399,2.8),.03);assert.equal(l.height(-.6+.28,2.8+.28),.03);assert.equal(l.height(-.6+.29,2.8+.29),0);
  assert.equal(l.height(-.99,2.41),0,'its square\'s corner is the counter');
  // An ellipse 1 m by 0.5 m.
  assert.equal(l.height(1.19,2.75),.04);assert.equal(l.height(.7,2.99),.04);assert.equal(l.height(1.15,2.6),0);
  assert.ok(round([0,0,2,2],1,0)&&round([0,0,2,2],0,1)&&!round([0,0,2,2],.29,.29));
  // The editor's analysis sees the same floor: a cell's surface is the deck's only inside the ellipse.
  const id=(x,z)=>l.cellIndex(x,z);
  assert.equal(a.groundH[id(-.6,2.8)],Math.fround(.03));assert.equal(a.groundH[id(-.98,2.42)],0);
  assert.deepEqual(a.issues,[]);
  // A hotdog steps up onto a low one and walks across.
  assert.ok(l.canStep([-.6,FLOOR,2.36],[-.6,FLOOR,2.41]));
  assert.equal(stringify(normalize(stringify(map))),stringify(map));
});
test('the dojo is a round bamboo ring a metre across by the front of the counter, eight starts on it facing the middle, chopstick posts round it, a gong and a stand of steamers',()=>{
  const text=read('../../assets/maps/kung-fu-dojo.json'),map=normalize(text),l=compile(map);
  assert.equal(stringify(map),text);
  assert.deepEqual([map.name,map.mode,map.theme,map.version,map.pickups.length],['Kung Fu Dojo','kungfu','kitchen',2,0]);
  const [ring]=map.decks,[x0,z0,x1,z1]=ring.rect,c=[(x0+x1)/2,(z0+z1)/2],r=(x1-x0)/2;
  assert.equal(ring.style,'bamboo');assert.equal(+(x1-x0).toFixed(6),+(z1-z0).toFixed(6),'a circle');
  assert.ok(r>=.45&&r<=.5&&ring.height<=.035,'about a metre across, low enough to step onto');
  assert.ok(z0-map.bounds[1]<=.12,'a throw off its front goes off the counter');
  assert.equal(map.starts.length,8);
  for(const start of map.starts){
    const [x,z]=start.p,dx=c[0]-x,dz=c[1]-z,ahead=[-Math.sin(start.yaw),-Math.cos(start.yaw)];
    assert.ok(Math.hypot(dx,dz)<r-.1,`${start.p} on the ring`);
    assert.ok((ahead[0]*dx+ahead[1]*dz)/Math.hypot(dx,dz)>.999,`${start.p} faces the middle`);
    assert.equal(l.ground(x,z),FLOOR+ring.height);
  }
  const posts=map.blocks.filter(b=>b.style==='bamboo');
  assert.equal(posts.length,4);
  for(const post of posts){const p=[(post.rect[0]+post.rect[2])/2,(post.rect[1]+post.rect[3])/2];
    assert.ok(Math.hypot(p[0]-c[0],p[1]-c[1])-.03*Math.SQRT2-.02>r,`${p}: the post and the room round it are off the ring`);assert.ok(post.height>.15,'taller than a hotdog')}
  assert.equal(map.blocks.filter(b=>b.style==='gong').length,1);
  // The stand: three steamers stacked, each a step up from the one under it.
  const tiers=map.decks.slice(1).sort((a,b)=>a.height-b.height);
  assert.equal(tiers.length,3);tiers.forEach((t,i)=>{assert.equal(t.style,'bamboo');assert.ok(t.height-(i?tiers[i-1].height:0)<=.045)});
  assert.deepEqual(analyze(l).issues,[]);
});
test('the kitchen holds the bundled Forts map: its bases and its microwave are inside the walls the game builds round it',()=>{
  const map=normalize(read('../../assets/maps/pillow-fort.json')),fort=fortress(map);
  // The game's walls (src/fridge.rs, Room::fitting): 0.7 m past the counter's ends and 0.07 m behind its back, as for the bundled siege map, up to ROOM.
  const walls=[Math.min(-2.5,Math.max(ROOM[0],map.bounds[0])-.7),Math.max(2.5,Math.min(ROOM[1],map.bounds[2])+.7),Math.max(3.7,Math.min(ROOM[2],map.bounds[3])+.07)];
  assert.deepEqual(walls.map(v=>+v.toFixed(6)),[-3.7,3.7,4.07]);
  const microwave=map.blocks.find(block=>block.style==='microwave').rect;
  for(const r of [...fort.bases,microwave])assert.ok(r[0]>walls[0]+.04&&r[2]<walls[1]-.04&&r[3]<walls[2]-.04,`${r} is inside the walls`);
  // Its microwave (a full-size one, 62 cm wide, 48 deep and 36 tall) stands against the back of the middle, its bases at either end, both about 0.7 m
  // inside the side walls.
  assert.deepEqual(fort.bases,[[-3,2.7,-2.5,3.5],[2.5,2.7,3,3.5]]);assert.deepEqual(microwave,[-.31,3.52,.31,4]);
});
test('analysis warns about a counter bigger than the largest kitchen the game builds',()=>{
  const codes=bounds=>analyze(compile(normalize({format:'hotdog-map',version:1,bounds,starts:[1,2,3,4,5].map(i=>({p:[bounds[0]+.1*i,bounds[1]+.1]})),spawns:[{p:[bounds[0]+.5,bounds[1]+.5]}]}))).issues.map(issue=>issue.code);
  assert.deepEqual(ROOM,[-4.5,4.5,5]);
  // The counters the old kitchen held, and the ones that went through its walls (2.5 m out, 3.7 m back), all fit now; so does the largest.
  for(const inside of [[-1.8,2.27,1.8,3.63],[-2.45,1,2.45,3.65],[-2.5,2.27,1.8,3.63],[-1.8,2.27,2.5,3.63],[-1.8,2.27,1.8,3.7],[-3,2.2,3,4],[-4.5,1,4.5,5]])assert.deepEqual(codes(inside),[],`${inside}`);
  for(const beyond of [[-4.6,2.27,1.8,3.63],[-1.8,2.27,4.6,3.63],[-1.8,2.27,1.8,5.1],[-20,2.2,20,4]])assert.deepEqual(codes(beyond),['beyond-room'],`${beyond}`);
  const issue=analyze(compile(normalize({format:'hotdog-map',version:1,bounds:[-6,2.2,6,4],starts:[{p:[0,3]}],spawns:[{p:[1,3]}]}))).issues.find(issue=>issue.code==='beyond-room');
  assert.deepEqual([issue.severity,issue.kind,issue.index],['warning','bounds',-1]);assert.match(issue.message,/largest kitchen the game builds.*x = -4\.5 to 4\.5 and up to z = 5/);
});

test('a pickup holds a gun with its rounds if it says them or a box of ammunition without a count, and a map may say how long its pickups take to come back',()=>{
  assert.deepEqual([WEAPONS[AMMO_BOX],WEAPONS.length,RESPAWN],['Ammo',8,[20,12]]);
  const map=normalize(edit(m=>{m.pickups=[{p:[0,3],weapon:0,ammo:50},{p:[.5,3],weapon:6,ammo:30}]}));
  assert.deepEqual(map.pickups,[{p:[0,3],weapon:0},{p:[.5,3],weapon:6,ammo:30}]);assert.equal('respawn' in map,false,'no respawn unless the map gives one');
  assert.match(stringify(map),/\{"p": \[0, 3\], "weapon": 0\}/);assert.deepEqual(normalize(stringify(map)),map);
  // A gun's rounds are the game's rules' (kitchen-sim magazine.rs PICKUP): a map may leave them out, and is written back as it was; a count it gives is
  // still a count.
  const bare=normalize(edit(m=>{m.pickups=[{p:[0,3],weapon:2}]}));
  assert.deepEqual(bare.pickups,[{p:[0,3],weapon:2}]);assert.match(stringify(bare),/\{"p": \[0, 3\], "weapon": 2\}/);assert.deepEqual(normalize(stringify(bare)),bare);
  assert.throws(()=>normalize(edit(m=>{m.pickups=[{p:[0,3],weapon:2,ammo:0}]})),/ammo/,'a count, if any, from 1');
  const timed=normalize(edit(m=>{m.respawn=[30,8.5]}));assert.deepEqual(timed.respawn,[30,8.5]);assert.deepEqual(Object.keys(timed).slice(0,8),['format','version','name','theme','mode','respawn','bounds','nav']);
  assert.deepEqual(normalize(stringify(timed)),timed);
  for(const bad of [[0,10],[10,601],[10],'20',[20,'x']])assert.throws(()=>normalize(edit(m=>{m.respawn=bad})),/respawn/,JSON.stringify(bad));
});

// Every map the game ships: canonical, without a problem the editor would list (Counter Siege v2's three unreachable decks aside, which it has always had), and
// with its loose objects clear of each other and of everything that would spoil a round's start: none over the counter's edge, on a deck or a ramp (unless it
// stands on top of the deck), in a block, on a start, a zombie spawn or a pickup. (Footprints are the shapes' outlines, presets.mjs's PRESETS, turned by their yaw.)
test('every bundled map is canonical and problem-free, and its loose objects stand clear at the start of a round',()=>{
  const outline=kind=>{let x=0,z=0;for(const part of PRESETS[kind]().parts){if(part.type==='box'){x=Math.max(x,Math.abs(part.center[0])+part.half[0]);z=Math.max(z,Math.abs(part.center[2])+part.half[2])}else{const r=Math.max(...part.profile.map(q=>q[0]))+part.radius;x=Math.max(x,r);z=Math.max(z,r)}}return [x,z]};
  const corners=({p,yaw,kind})=>{const [hx,hz]=outline(kind),c=Math.cos(yaw),s=Math.sin(yaw);return [[-hx,-hz],[hx,-hz],[hx,hz],[-hx,hz]].map(([u,w])=>[p[0]+c*u+s*w,p[1]-s*u+c*w])};
  const edges=q=>q.map((a,i)=>{const b=q[(i+1)%q.length];return [b[1]-a[1],a[0]-b[0]]});
  const apart=(a,b)=>[...edges(a),...edges(b)].some(([x,z])=>{const u=a.map(p=>p[0]*x+p[1]*z),v=b.map(p=>p[0]*x+p[1]*z);return Math.max(...u)<=Math.min(...v)+1e-9||Math.max(...v)<=Math.min(...u)+1e-9});
  const quad=r=>[[r[0],r[1]],[r[2],r[1]],[r[2],r[3]],[r[0],r[3]]];
  const touches=(q,c,r)=>edges(q).every(([x,z],i)=>(c[0]-q[i][0])*x+(c[1]-q[i][1])*z<=0)||q.some((a,i)=>{const b=q[(i+1)%4],dx=b[0]-a[0],dz=b[1]-a[1],t=Math.max(0,Math.min(1,((c[0]-a[0])*dx+(c[1]-a[1])*dz)/(dx*dx+dz*dz)));return Math.hypot(a[0]+dx*t-c[0],a[1]+dz*t-c[1])<r});
  const names=readdirSync(new URL('../../assets/maps/',import.meta.url)).filter(name=>name.endsWith('.json')).sort();
  assert.deepEqual(names,['breakfast-rush.json','counter-siege-v2.json','counter-siege.json','horde-night.json','jungle-fort.json','kung-fu-dojo.json','midnight-snack.json','moonlit-fort.json','overgrown-pantry.json','pillow-fort.json','the-whole-kitchen-forts.json','the-whole-kitchen.json','toaster-alley.json','treetop-decks.json']);
  for(const name of names){
    const text=read(`../../assets/maps/${name}`),map=normalize(text),issues=analyze(compile(map)).issues.map(i=>i.code);
    assert.equal(stringify(map),text,`${name} is canonical`);
    assert.deepEqual(issues,name==='counter-siege-v2.json'?['deck-unreachable','deck-unreachable','deck-unreachable']:[],`${name}: ${issues}`);
    const loose=map.props.filter(prop=>LOOSE.includes(prop.kind)).map(prop=>({...prop,q:corners(prop)}));
    loose.forEach((a,i)=>{const tag=`${name}: ${a.kind} ${i} at ${a.p}`,[x0,z0,x1,z1]=map.bounds;
      assert.ok(a.q.every(([x,z])=>x>=x0&&x<=x1&&z>=z0&&z<=z1),`${tag} hangs over the counter's edge`);
      // (A round deck is its ellipse: an object is clear of it when, squeezed so that the ellipse is the unit circle, no edge of its box comes inside the circle.)
      const off=(d,q)=>{if(d.style!==ROUND)return apart(q,quad(d.rect));
        const c=[(d.rect[0]+d.rect[2])/2,(d.rect[1]+d.rect[3])/2],rx=(d.rect[2]-d.rect[0])/2,rz=(d.rect[3]-d.rect[1])/2,u=q.map(([x,z])=>[(x-c[0])/rx,(z-c[1])/rz]);
        return u.every((p,i)=>{const b=u[(i+1)%u.length],dx=b[0]-p[0],dz=b[1]-p[1],t=Math.max(0,Math.min(1,-(p[0]*dx+p[1]*dz)/(dx*dx+dz*dz)));return Math.hypot(p[0]+dx*t,p[1]+dz*t)>=1})};
      const on=d=>Math.abs(a.base-d.height)<1e-3&&a.q.every(([x,z])=>x>=d.rect[0]&&x<=d.rect[2]&&z>=d.rect[1]&&z<=d.rect[3]&&(d.style!==ROUND||round(d.rect,x,z)));
      map.decks.forEach((d,j)=>assert.ok(off(d,a.q)||on(d)||a.base>d.height+1e-3&&map.decks.some(e=>e!==d&&on(e)),`${tag} overlaps deck ${j}`));
      for(const [kind,list] of [['ramp',map.ramps],['block',map.blocks],['box',map.props.filter(prop=>prop.kind==='box')]])list.forEach((r,j)=>assert.ok(apart(a.q,quad(r.rect)),`${tag} overlaps ${kind} ${j}`));
      for(const [kind,list,r] of [['start',map.starts,.05],['spawn',map.spawns,.04],['pickup',map.pickups,.03]])list.forEach((item,j)=>assert.ok(!touches(a.q,item.p,r),`${tag} is on ${kind} ${j}`));
      loose.forEach((b,j)=>assert.ok(j<=i||Math.abs(a.base-b.base)>=.005||apart(a.q,b.q),`${tag} overlaps ${b.kind} ${j}`))});
  }
});

// The Rust client must build byte-identical grids, and the Rust server write byte-identical canonical text. Both test suites compare against this file.
test('grid fingerprints match the committed golden file shared with the Rust tests',()=>{
  const golden=new URL('./fixtures/map-golden.json',import.meta.url);
  const actual={};
  // (The Whole Kitchen has a sink, with its rim and the ramps out of it.)
  for(const name of ['../../assets/maps/counter-siege.json','./fixtures/awkward.json','../../assets/maps/pillow-fort.json','../../assets/maps/the-whole-kitchen-forts.json','../../assets/maps/kung-fu-dojo.json']){
    const l=compile(normalize(read(name)));
    actual[name.split('/').at(-1)]={cols:l.cols,rows:l.rows,grid:gridHash(l),ground:groundHash(l)};
  }
  // A Forts map while its forcefield is up: the field is one more block.
  for(const name of ['pillow-fort.json','the-whole-kitchen-forts.json']){const l=fielded(normalize(read(`../../assets/maps/${name}`)));actual[`${name} fielded`]={cols:l.cols,rows:l.rows,grid:gridHash(l),ground:groundHash(l)}}
  // The canonical text, which the Rust server writes (map.rs `to_text`) and sends its clients: an FNV-1a hash of its UTF-8 bytes.
  const textHash=text=>{let h=0x811c9dc5;for(const byte of Buffer.from(text,'utf8'))h=Math.imul(h^byte,0x01000193)>>>0;return h.toString(16).padStart(8,'0')};
  actual.canonical=Object.fromEntries(['./fixtures/awkward.json','./fixtures/uncanonical.json','../../assets/maps/pillow-fort.json','../../assets/maps/the-whole-kitchen-forts.json','../../assets/maps/kung-fu-dojo.json'].map(name=>[name.split('/').at(-1),textHash(stringify(normalize(read(name))))]));
  if(process.env.UPDATE_GOLDEN==='1'||!existsSync(golden))writeFileSync(golden,JSON.stringify(actual,null,2)+'\n');
  assert.deepEqual(actual,JSON.parse(readFileSync(golden,'utf8')));
});

// The editor draws the loose objects from presets.mjs; the game simulates and draws them from props.rs's shapes. Both test suites hold their
// shapes to this record (UPDATE_GOLDEN=1 rewrites it after a deliberate change, which props.rs must then follow).
test('the loose objects\' shapes match the golden file shared with the Rust tests',()=>{
  const golden=new URL('./fixtures/presets-golden.json',import.meta.url),actual=Object.fromEntries(LOOSE.map(kind=>[kind,PRESETS[kind]()]));
  if(process.env.UPDATE_GOLDEN==='1'||!existsSync(golden))writeFileSync(golden,`{\n${Object.entries(actual).map(([kind,shape])=>`  ${JSON.stringify(kind)}: ${JSON.stringify(shape)}`).join(',\n')}\n}\n`);
  assert.deepEqual(actual,JSON.parse(readFileSync(golden,'utf8')));
});

// Version 2: a sink, its boards and its rim, as map.rs's tests have them (the golden file keeps the grids equal).
test('a sink is a basin: its floor is ground below the counter, its rim keeps routes out but for its boards, and its sides stop shots',()=>{
  const level=compile(normalize(read('../../assets/maps/the-whole-kitchen.json'))),[sx0,sz0,sx1,sz1]=level.map.sinks[0].rect,depth=level.map.sinks[0].depth,bottom=FLOOR-depth,lane=3.48;
  assert.ok(Math.abs(level.ground(0,lane)-bottom)<1e-12&&level.ground(-.5,lane)===FLOOR&&level.ground(-.2,lane)>bottom&&level.ground(-.2,lane)<FLOOR);
  assert.ok(level.canStep([sx0-.01,FLOOR,3.3],[sx0+.01,FLOOR,3.3])&&!level.canStep([sx0+.01,bottom,3.3],[sx0-.01,bottom,3.3]));
  assert.ok(!level.clear(sx0,3.3)&&!level.clear(0,sz0)&&!level.clear(0,sz1)&&level.clear(0,lane)&&level.clear(sx0-.02,lane)&&level.clear(0,3.3));
  assert.ok(level.path([-2,2.5],[0,3.3]).some(p=>Math.abs(p[1]-lane)<.07&&p[0]<sx0+.06));
  const foot=level.climbGoal([.1,bottom,3.3],[2,FLOOR,3]),top=level.climbGoal([.1,bottom+.01,lane],[2,FLOOR,3]),beside=level.climbGoal([-.16,bottom,3.405],[-2,FLOOR,3]);
  assert.ok(foot[0]>0&&foot[0]<.2&&Math.abs(foot[2]-lane)<.07&&top[0]>sx1&&top[1]===FLOOR&&beside[0]>-.14&&beside[0]<-.1,JSON.stringify([foot,top,beside]));
  assert.ok(Math.abs(level.cover([0,bottom+.05,3.3],[1,0,0])-sx1)<1e-12&&level.cover([0,bottom+.05,3.3],[0,1,0])===RANGE);
  assert.ok(Math.abs(level.cover([0,FLOOR+.3,3],[0,-.6,.8])-(.3+depth)/.6)<1e-12&&level.sunk(0,3.3)&&!level.sunk(0,3)&&level.sunken());
  assert.deepEqual(level.spawnPosition(0)[1],FLOOR);
});

