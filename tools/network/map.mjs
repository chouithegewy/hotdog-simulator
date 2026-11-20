// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Map format, collision and navigation shared by the server, its tests and the browser map editor.
// No I/O and no dependencies: this file runs unchanged in Node and in the browser.
// crates/kitchen-sim/src/map.rs implements the same rules for the game client; fixtures/map-golden.json keeps them in step.

// VERSION is the newest map version this build reads. A map is written as version 1 unless it uses something version 2 added (`versionOf`): sinks, the fitted
// cabinets, the counter things among KITS after the first two (FIRST_KITS), the styles after the first nine (FIRST_STYLES), a ramp with a style or one that starts
// below the counter, and the bin. So a map that
// needs none of them stays byte for byte what it was and builds that read only version 1 still read it; one that does is refused by them with a reason.
export const FLOOR=.9025,FORMAT='hotdog-map',VERSION=2;
// The materials of blocks, decks, ramps and boxes; version 1 knew the first nine. Bamboo (ROUND) is also a shape: a deck of bamboo is round (a steamer's lid:
// the ellipse inside its rect is its floor, `round`), and a slim block of it is a chopstick standing in its holder.
export const STYLES=['steel','stone','wood','dark','white','red','green','yellow','metal','bamboo'],FIRST_STYLES=9,ROUND='bamboo';
// Appliances and the things that stand on a kitchen counter: blocks (a box each to walk round, shoot at and stand on) that the game draws as what they are, with a
// front on their `facing` side (the griddle's is the side with its thermostat dial: it is switched on and off, heats and burns, crates/kitchen-sim/src/griddle.rs;
// the gong is a wok hung in a frame of long chopsticks, struck at a knockout in Kung Fu). Version 1 knew the first two.
export const KITS=['microwave','toaster','kettle','knife-block','canister','jar','cereal','paper-towel','dish-rack','spice-rack','fruit-bowl','faucet','griddle','gong'],FIRST_KITS=2;
// A sink's depth below the counter when the map does not give one, and the deepest a sink (or the foot of a ramp down into one) may go.
export const SINK_DEPTH=.18,SINK_MAX=.5;
// Loose objects: props that can be knocked about. The game simulates them as rigid bodies (crates/kitchen-sim/src/props.rs; their shapes are in presets.mjs).
export const LOOSE=['bowl','crate','pillow','napkin','chair','tray','cereal','cup','plate','tin','board','dish','roll','salt','pepper','breadbox'];
// A kind of loose object that maps once called by another name: a blanket is a napkin now (map.rs LOOSE_FORMERLY).
export const LOOSE_FORMERLY={blanket:'napkin'};
export const RISES=['+x','-x','+z','-z'];
// How the game dresses and lights the map; it changes nothing about collision or navigation.
export const THEMES=['kitchen','night','jungle'];
// What a map was made for: 'siege' (the zombie wave, then the free for all), 'forts' (two teams build a fortress each side of a forcefield, then fight over the
// bacon) or 'kungfu' (hands and feet only, in a ring). It is only the mode the map is played in when nothing else says: the mode is the game's (the lobby, the
// page's ?mode=, the server's --mode), and a map may be played in any mode it can hold (kitchen-sim rules.rs `Map::refuses`; `refuses` here).
export const MODES=['siege','forts','kungfu'];
// The two teams of a Forts map, in the order the game numbers them (0 and 1), and what they are called.
export const TEAMS=['mustard','ketchup'],TEAM_NAMES=['Mustard Eaters','Ketchup Connoisseurs'];
// What a map marks out for Forts, in its `zones` (layout any map may carry: one with all of them can be played as Forts): each team's base (an area, with its
// team), the forcefield (a thin area right across the map) and the bacon's spot (a point).
export const ZONES=['base','field','bacon'];
// Seconds a Forts round's build phase lasts (a rule of the game, set by the server's --build or the page's ?build=: a map made for Forts may say `build`,
// which is read and written back but passed over), and the hotdogs a Forts round has (three a side).
export const BUILD=150,FORT_PLAYERS=6;
// What a pickup holds, by number: 1 to 7 a gun; 0 (AMMO_BOX; there are no bare hands, so the number was free) a box of ammunition, which refills the guns
// its taker holds and has no count of its own. A gun's pickup may say its rounds (`ammo`), and a map how long its pickups take to come back (`respawn`):
// both are read and written back, but the game's rules say what a pickup gives and when it is back (kitchen-sim magazine.rs PICKUP and COMEBACK, the same
// in every gun mode: a magazine, the power weapons fewer; RESPAWN[0] seconds for a gun, RESPAWN[1] for a box, the power weapons 30 to 60).
export const WEAPONS=['Ammo','Mustard pistol','Ketchup sniper','Hot sauce thrower','Relish bazooka','Bug spray','Syrup rifle','Mayo squirter'],AMMO_BOX=0,RESPAWN=[20,12];
export const LIMITS={extent:50,height:3,cells:1<<20,starts:16,spawns:64,pickups:256,blocks:4096,decks:1024,ramps:1024,sinks:16,props:4096,zones:16};
// Movement constants, in metres: the tallest ledge a hotdog steps onto, how close its body gets to a block,
// how far above a block's top it must be to pass over, and how far a shot travels.
export const STEP=.045,MARGIN=.03,LIP=.005,RANGE=12;
const CLIMB=.12,FOOT=.03,HEAD=.15,PLAYERS=5;
// The kitchen the game builds round the counter grows with it: its side walls stand 0.7 m past the counter's ends and its back wall just behind it (never
// nearer in than x = -2.5 and 2.5 and z = 3.7, the bundled counter's room), up to the largest kitchen, which holds a counter inside ROOM, [min x, max x, max z].
// (The room is open towards -z, where the fridge stands some two metres off.) crates/kitchen-sim/src/map.rs has the same ROOM.
export const ROOM=[-4.5,4.5,5];

export class MapError extends Error {constructor(message){super(message);this.name='MapError'}}

// Coordinates are kept on a 0.1 mm lattice so every implementation derives the same navigation grid from integers.
const UNIT=1e4,quantum=v=>Math.floor(v*UNIT+.5),trim=v=>quantum(v)/UNIT;
const fail=message=>{throw new MapError(message)};
const number=(v,path,min,max,fallback)=>{
  if(v===undefined&&fallback!==undefined)return fallback;
  if(typeof v!=='number'||!Number.isFinite(v))fail(`${path} must be a number`);
  if(v<min||v>max)fail(`${path} must be between ${min} and ${max}`);
  return trim(v);
};
const whole=(v,path,min,max)=>{if(!Number.isInteger(v)||v<min||v>max)fail(`${path} must be a whole number from ${min} to ${max}`);return v};
const point=(v,path)=>{if(!Array.isArray(v)||v.length!==2)fail(`${path} must be [x, z]`);return v.map((c,i)=>number(c,`${path}[${i}]`,-LIMITS.extent,LIMITS.extent))};
const rect=(v,path)=>{
  if(!Array.isArray(v)||v.length!==4)fail(`${path} must be [minX, minZ, maxX, maxZ]`);
  const r=v.map((c,i)=>number(c,`${path}[${i}]`,-LIMITS.extent,LIMITS.extent));
  if(r[2]-r[0]<.001||r[3]-r[1]<.001)fail(`${path} is inside out or empty; expected [minX, minZ, maxX, maxZ]`);
  return r;
};
const choice=(v,path,allowed,fallback)=>{if(v===undefined&&fallback!==undefined)return fallback;if(!allowed.includes(v))fail(`${path} must be one of ${allowed.join(', ')}`);return v};
const tall=(v,path,fallback,min=.01)=>number(v,path,min,LIMITS.height,fallback);
// One entry per kind of object, in the order they are written to a file.
const ITEMS={
  // A start may name the team it belongs to: a Forts map's starts all do.
  starts:(s,path)=>{const start={p:point(s.p,`${path}.p`),yaw:number(s.yaw,`${path}.yaw`,-7,7,0)};if(s.team!==undefined&&s.team!==null)start.team=choice(s.team,`${path}.team`,TEAMS);return start},
  spawns:(s,path)=>({p:point(s.p,`${path}.p`)}),
  // A box of ammunition (weapon 0) has no count: it is written without one. A gun's count is optional (the game's rules say what it gives), and kept
  // when given.
  pickups:(p,path)=>{const weapon=whole(p.weapon,`${path}.weapon`,0,WEAPONS.length-1),at=point(p.p,`${path}.p`);return weapon===AMMO_BOX||p.ammo===undefined||p.ammo===null?{p:at,weapon}:{p:at,weapon,ammo:whole(p.ammo,`${path}.ammo`,1,999)}},
  blocks:(b,path)=>{
    const block={rect:rect(b.rect,`${path}.rect`),height:tall(b.height,`${path}.height`,.25,.02),style:choice(b.style,`${path}.style`,[...STYLES,...KITS],'steel')};
    if(KITS.includes(block.style))block.facing=choice(b.facing,`${path}.facing`,RISES,'-z');
    return block;
  },
  decks:(d,path)=>({rect:rect(d.rect,`${path}.rect`),height:tall(d.height,`${path}.height`,.28),style:choice(d.style,`${path}.style`,STYLES,'metal')}),
  // A ramp's foot is below the counter (down to -SINK_MAX) only in a sink: a board propped in it, the way out. A ramp may name a style: the game then draws it as
  // a plank of that material rather than the metal ramp with yellow edges.
  ramps:(r,path)=>{
    const ramp={rect:rect(r.rect,`${path}.rect`),base:number(r.base,`${path}.base`,-SINK_MAX,LIMITS.height,0),height:tall(r.height,`${path}.height`,.28),rise:choice(r.rise,`${path}.rise`,RISES)};
    if(r.style!==undefined&&r.style!==null)ramp.style=choice(r.style,`${path}.style`,STYLES);
    if(ramp.height-ramp.base<.005)fail(`${path} must rise: height has to be above base`);
    return ramp;
  },
  // A basin `depth` metres deep sunk into the counter over `rect`: its floor is ground like any other and its sides a ledge too tall to step up (see Level).
  sinks:(s,path)=>({rect:rect(s.rect,`${path}.rect`),depth:number(s.depth,`${path}.depth`,.05,SINK_MAX,SINK_DEPTH)}),
  props:(p,path)=>{
    const kind=choice(LOOSE_FORMERLY[p.kind]??p.kind,`${path}.kind`,['box','turret',...LOOSE,'bin'],'box');
    // A bin stands on the kitchen floor beside the counter, turned by a yaw: scenery only.
    if(kind==='bin')return {kind,p:point(p.p,`${path}.p`),yaw:number(p.yaw,`${path}.yaw`,-7,7,0)};
    // A turret and every loose object stand at a point, at a height, turned by a yaw.
    return kind!=='box'
    ?{kind,p:point(p.p,`${path}.p`),base:number(p.base,`${path}.base`,0,LIMITS.height,0),yaw:number(p.yaw,`${path}.yaw`,-7,7,0)}
    :{kind:'box',rect:rect(p.rect,`${path}.rect`),base:number(p.base,`${path}.base`,0,LIMITS.height,0),height:tall(p.height,`${path}.height`,.1,.005),style:choice(p.style,`${path}.style`,STYLES,'wood')};
  },
  zones:(z,path)=>{
    const kind=choice(z.kind,`${path}.kind`,ZONES);
    return kind==='bacon'?{kind,p:point(z.p,`${path}.p`)}:kind==='field'?{kind,rect:rect(z.rect,`${path}.rect`)}:{kind,team:choice(z.team,`${path}.team`,TEAMS),rect:rect(z.rect,`${path}.rect`)};
  },
};
export const KINDS=Object.keys(ITEMS);
const REQUIRED={starts:1,spawns:1};

/** Throws unless a map may hold this many objects of a kind. */
export function checkCount(kind,count){
  if(count<(REQUIRED[kind]??0))fail(`A map needs at least one ${kind.replace(/s$/,'')}`);
  if(count>LIMITS[kind])fail(`${kind} holds ${count} entries; the limit is ${LIMITS[kind]}`);
}
/** Checks one object of the given kind and returns it in canonical form. */
export function normalizeItem(kind,item,path=kind){
  if(!ITEMS[kind])fail(`Unknown kind of object: ${kind}`);
  if(!item||typeof item!=='object'||Array.isArray(item))fail(`${path} must be an object`);
  return ITEMS[kind](item,path);
}
/**
 * Checks the map-wide settings and returns {name, theme, mode, bounds, nav} in canonical form; a Forts map also has `build`, the seconds its build phase lasts,
 * and a map that gives it `respawn`: [seconds a gun takes to come back, seconds a box of ammunition takes], each 1 to 600 (RESPAWN when it is left out).
 */
export function normalizeSettings(input){
  const name=typeof input.name==='string'&&input.name.trim()?input.name.trim().slice(0,64):'Untitled map';
  const theme=choice(input.theme,'theme',THEMES,'kitchen'),mode=choice(input.mode,'mode',MODES,'siege');
  const rules=mode==='forts'?{build:number(input.build,'build',5,900,BUILD)}:{};
  if(input.respawn!==undefined&&input.respawn!==null){if(!Array.isArray(input.respawn)||input.respawn.length!==2)fail('respawn must be [weapon seconds, ammo seconds]');rules.respawn=input.respawn.map((v,i)=>number(v,`respawn[${i}]`,1,600))}
  // Fitted cabinets: doors and drawers down the counter's front and ends, and dressed wall cupboards. Scenery only; written only when on.
  if(input.cabinets!==undefined&&input.cabinets!==null){if(typeof input.cabinets!=='boolean')fail('cabinets must be true or false');if(input.cabinets)rules.cabinets=true}
  const bounds=rect(input.bounds,'bounds');
  if(bounds[2]-bounds[0]<.2||bounds[3]-bounds[1]<.2)fail('bounds must be at least 0.2 m on each side');
  const nav={cell:number(input.nav?.cell,'nav.cell',.01,.5,.04),clearance:number(input.nav?.clearance,'nav.clearance',0,.2,.035)};
  const cells=Math.ceil((quantum(bounds[2])-quantum(bounds[0]))/quantum(nav.cell))*Math.ceil((quantum(bounds[3])-quantum(bounds[1]))/quantum(nav.cell));
  if(cells>LIMITS.cells)fail(`The navigation grid would have ${cells} cells; the limit is ${LIMITS.cells}. Enlarge nav.cell or shrink bounds`);
  return {name,theme,mode,...rules,bounds,nav};
}

/** Checks a map and returns it in canonical form: defaults filled in, unknown keys dropped, coordinates trimmed. */
export function normalize(input){
  if(typeof input==='string'){try{input=JSON.parse(input)}catch(error){fail(`Not valid JSON: ${error.message}`)}}
  if(!input||typeof input!=='object'||Array.isArray(input))fail('A map must be a JSON object');
  if(input.format!==FORMAT)fail(`Unknown format ${JSON.stringify(input.format)}; expected "${FORMAT}"`);
  if(input.version!==1&&input.version!==VERSION)fail(`Unsupported map version ${JSON.stringify(input.version)}; this build reads versions 1 and ${VERSION}`);
  const map={format:FORMAT,version:VERSION,...normalizeSettings(input)};
  for(const kind of KINDS){
    const items=input[kind]??[];
    if(!Array.isArray(items))fail(`${kind} must be a list`);
    checkCount(kind,items.length);
    map[kind]=items.map((item,index)=>normalizeItem(kind,item,`${kind}[${index}]`));
  }
  map.version=versionOf(map);
  return map;
}
/** The version a map is written as (see VERSION): 2 when it uses anything version 1 did not have, 1 otherwise. */
export function versionOf(map){
  const late=style=>STYLES.indexOf(style)>=FIRST_STYLES;
  const newer=!!map.cabinets||(map.sinks??[]).length>0||map.blocks.some(b=>KITS.indexOf(b.style)>=FIRST_KITS||late(b.style))||map.decks.some(d=>late(d.style))||map.ramps.some(r=>r.base<0||r.style!==undefined)
    ||map.props.some(p=>p.kind==='bin'||p.kind==='box'&&late(p.style));
  return newer?VERSION:1;
}

const inline=v=>Array.isArray(v)?`[${v.map(inline).join(', ')}]`:v&&typeof v==='object'?`{${Object.entries(v).map(([key,value])=>`${JSON.stringify(key)}: ${inline(value)}`).join(', ')}}`:JSON.stringify(v);
/** Canonical text for a normalised map: one object per line, so maps diff cleanly. */
export function stringify(map){
  // The version is what the map needs, and a map without a sink says nothing of sinks, as version 1 did not.
  const out={...map,version:versionOf(map)};
  if(!out.sinks?.length)delete out.sinks;
  if(!out.cabinets)delete out.cabinets;
  const lines=Object.entries(out).map(([key,value])=>{
    const expanded=Array.isArray(value)&&value.length&&typeof value[0]==='object'&&!Array.isArray(value[0]);
    return `  ${JSON.stringify(key)}: `+(expanded?`[\n${value.map(item=>'    '+inline(item)).join(',\n')}\n  ]`:inline(value));
  });
  return `{\n${lines.join(',\n')}\n}\n`;
}

/** A small valid map to start from. */
export function blank(name='Untitled map'){
  return normalize({format:FORMAT,version:VERSION,name,bounds:[-1.8,2.27,1.8,3.63],starts:[{p:[.83,3.23],yaw:.9}],spawns:[{p:[-1.23,3.02]}]});
}

function rayBox(ox,oy,oz,dx,dy,dz,x0,y0,z0,x1,y1,z1){
  let near=0,far=Infinity,a,b;
  if(Math.abs(dx)<1e-8){if(ox<x0||ox>x1)return Infinity}else{a=(x0-ox)/dx;b=(x1-ox)/dx;near=Math.max(near,Math.min(a,b));far=Math.min(far,Math.max(a,b));if(near>far)return Infinity}
  if(Math.abs(dy)<1e-8){if(oy<y0||oy>y1)return Infinity}else{a=(y0-oy)/dy;b=(y1-oy)/dy;near=Math.max(near,Math.min(a,b));far=Math.min(far,Math.max(a,b));if(near>far)return Infinity}
  if(Math.abs(dz)<1e-8){if(oz<z0||oz>z1)return Infinity}else{a=(z0-oz)/dz;b=(z1-oz)/dz;near=Math.max(near,Math.min(a,b));far=Math.min(far,Math.max(a,b));if(near>far)return Infinity}
  return near;
}

/** Whether a map has the whole Forts layout in its zones: a base for each team, the field and the bacon's spot (kitchen-sim map.rs `fort_layout`). */
export const fortLayout=map=>{const zones=map.zones??[];return TEAMS.every(team=>zones.some(z=>z.kind==='base'&&z.team===team))&&zones.some(z=>z.kind==='field')&&zones.some(z=>z.kind==='bacon')};
/** The modes a map can be played in (kitchen-sim rules.rs `Map::modes`): the dojo Kung Fu alone; any other map the siege and Free For All, Forts where it has the
 * layout (or was made for Forts), and Fully Loaded (whose table the server keeps to Counter Siege). */
export const modes=map=>map.mode==='kungfu'?['kungfu']:['siege',...fortress(map)?['forts']:[],'ffa','cards'];
/**
 * What a map made for Forts, or any map with the whole Forts layout (`fortLayout`), says for Forts, with what it leaves out filled in, so that an unfinished
 * Forts map still plays (`analyze` reports what is missing). Null for any other map: one whose zones are not the whole layout cannot be played as Forts.
 * axis: 0 when the forcefield divides the map along x (it runs along z), 1 when along z; mid: the field's middle on that axis; field: its area;
 * bases: each team's area, by team number; sides: the side of the field each team is on (-1 below `mid`, 1 above); bacon: its spot [x, z];
 * starts: each team's starts ({p, yaw}); build: seconds the build phase lasts.
 * Without a field, a strip across the middle of the longer side stands in; without a base, the end eighth of the map on that team's side; without the bacon's spot, the
 * middle of the field. Starts that name no team go to the team whose side they are on, and a team with no start begins in the middle of its base.
 */
export function fortress(map){
  if(map.mode!=='forts'&&!fortLayout(map))return null;
  const [x0,z0,x1,z1]=map.bounds,zones=map.zones??[],given=zones.find(z=>z.kind==='field')?.rect;
  const axis=given?(given[2]-given[0]<=given[3]-given[1]?0:1):(x1-x0>=z1-z0?0:1),centre=axis?(z0+z1)/2:(x0+x1)/2;
  const field=given??(axis?[x0,centre-.03,x1,centre+.03]:[centre-.03,z0,centre+.03,z1]),mid=(field[axis]+field[axis+2])/2,side=p=>p[axis]<mid?-1:1;
  const low=map.bounds[axis],high=map.bounds[axis+2],end=(high-low)/8;
  const stand=t=>axis?[x0,t?high-end:low,x1,t?high:low+end]:[t?high-end:low,z0,t?high:low+end,z1];
  let bases=TEAMS.map(team=>zones.find(z=>z.kind==='base'&&z.team===team)?.rect??null);
  const middle=r=>[(r[0]+r[2])/2,(r[1]+r[3])/2];
  let sides=bases.map((r,t)=>r?side(middle(r)):0);
  // A base on neither side yet takes the side its rival has left (the low side for the first team when neither has one); two on one side are an error `analyze` reports.
  if(!sides[0])sides[0]=sides[1]?-sides[1]:-1;if(!sides[1])sides[1]=-sides[0];
  bases=bases.map((r,t)=>r??stand(sides[t]>0?1:0));
  const bacon=zones.find(z=>z.kind==='bacon')?.p??middle(field),named=map.starts.some(s=>s.team);
  const starts=TEAMS.map((team,t)=>{
    const own=map.starts.filter(s=>named?s.team===team:side(s.p)===sides[t]).map(s=>({p:s.p,yaw:s.yaw}));
    // Facing the field: yaw 0 looks along -z, and a quarter turn left along -x.
    return own.length?own:[{p:middle(bases[t]),yaw:axis?(sides[t]>0?0:Math.PI):(sides[t]>0?Math.PI/2:-Math.PI/2)}];
  });
  return {axis,mid,field,bases,sides,bacon,starts,build:map.build??BUILD};
}

/**
 * Where a shot from o along the unit vector d, once it is below the counter top over the sink at s[i..i+5] (rect, depth), meets the sink's side or floor:
 * Infinity when it is never below the counter top there, or leaves the basin by its open top.
 */
function basin(o,d,s,i){
  let start=0;
  if(o[1]>=FLOOR){if(d[1]>=0)return Infinity;start=(FLOOR-o[1])/d[1]}
  const x=o[0]+d[0]*start,z=o[2]+d[2]*start;
  if(!(x>=s[i]&&x<=s[i+2]&&z>=s[i+1]&&z<=s[i+3]))return Infinity;
  // From there it is inside the basin, which it leaves through the nearest face ahead of it.
  const low=[s[i],FLOOR-s[i+4],s[i+1]],high=[s[i+2],FLOOR,s[i+3]];
  let out=Infinity,top=false;
  for(let k=0;k<3;k++){
    if(Math.abs(d[k])<1e-8)continue;
    const t=((d[k]>0?high[k]:low[k])-o[k])/d[k];
    if(t<out){out=t;top=k===1&&d[1]>0}
  }
  return top?Infinity:Math.max(out,start);
}

const DIAGONAL=Math.SQRT2;
/** A map prepared for play: the blocked-cell grid plus flat arrays for the collision queries. */
export class Level {
  constructor(map){
    this.map=map;
    [this.x0,this.z0,this.x1,this.z1]=map.bounds;
    const cell=this.cell=map.nav.cell,cellU=quantum(cell),clearU=quantum(map.nav.clearance),x0U=quantum(this.x0),z0U=quantum(this.z0);
    const cols=this.cols=Math.max(1,Math.ceil((quantum(this.x1)-x0U)/cellU)),rows=this.rows=Math.max(1,Math.ceil((quantum(this.z1)-z0U)/cellU));
    // A cell is blocked when its centre lies within a block grown by the clearance. Doubling keeps half-cell centres integral.
    const blocked=this.blocked=new Uint8Array(cols*rows);
    const span=(low,high,origin,count)=>[Math.max(0,Math.ceil((2*low-2*origin-cellU)/(2*cellU))),Math.min(count-1,Math.floor((2*high-2*origin-cellU)/(2*cellU)))];
    const n=map.blocks.length,walls=this.walls=new Float64Array(n*5),solids=this.solids=new Float64Array(n*5);
    map.blocks.forEach((block,index)=>{
      const [bx0,bz0,bx1,bz1]=block.rect,[i0,i1]=span(quantum(bx0)-clearU,quantum(bx1)+clearU,x0U,cols),[j0,j1]=span(quantum(bz0)-clearU,quantum(bz1)+clearU,z0U,rows);
      if(i0<=i1)for(let j=j0;j<=j1;j++)blocked.fill(1,j*cols+i0,j*cols+i1+1);
      walls.set([bx0-MARGIN,bz0-MARGIN,bx1+MARGIN,bz1+MARGIN,FLOOR+(block.height+LIP)],index*5);
      solids.set([bx0,bz0,bx1,bz1,FLOOR+block.height],index*5);
    });
    // A sink's rim is blocked: the cells within the clearance of its edge (or within half a cell, should the cells be coarser), outside it and in, so that routes
    // neither cross a sink nor wander into it, except over the top of a ramp whose foot stands below the counter (a board propped in the sink), which is the way
    // in and out. Its floor inside the rim is clear, so a route can be found down there too. Doubled again: every test is integral.
    const reach=Math.max(clearU,cellU/2);
    const gaps=map.ramps.filter(ramp=>ramp.base<0).map(ramp=>{
      const [rx0,rz0,rx1,rz1]=ramp.rect.map(quantum),gap=[2*rx0,2*rz0,2*rx1,2*rz1];
      if(ramp.rise==='+x')gap[2]+=2*reach;else if(ramp.rise==='-x')gap[0]-=2*reach;else if(ramp.rise==='+z')gap[3]+=2*reach;else gap[1]-=2*reach;
      return gap;
    });
    for(const sink of map.sinks??[]){
      const [sx0,sz0,sx1,sz1]=sink.rect.map(quantum),[i0,i1]=span(sx0-reach,sx1+reach,x0U,cols),[j0,j1]=span(sz0-reach,sz1+reach,z0U,rows);
      if(i0>i1||j0>j1)continue;
      for(let j=j0;j<=j1;j++){
        const cz=2*z0U+(2*j+1)*cellU;
        for(let i=i0;i<=i1;i++){
          const cx=2*x0U+(2*i+1)*cellU,within=cx>2*(sx0+reach)&&cx<2*(sx1-reach)&&cz>2*(sz0+reach)&&cz<2*(sz1-reach);
          if(!within&&!gaps.some(g=>cx>=g[0]&&cx<=g[2]&&cz>=g[1]&&cz<=g[3]))blocked[j*cols+i]=1;
        }
      }
    }
    // rect, depth
    this.sinks=new Float64Array((map.sinks??[]).flatMap(sink=>[...sink.rect,sink.depth]));
    // rect, height, 1 when the deck is round (ROUND)
    this.decks=new Float64Array(map.decks.flatMap(deck=>[...deck.rect,deck.height,deck.style===ROUND?1:0]));
    // rect, base, top, axis (0 = x, 1 = z), direction of the rise along that axis
    this.ramps=new Float64Array(map.ramps.flatMap(ramp=>[...ramp.rect,ramp.base,ramp.height,ramp.rise[1]==='z'?1:0,ramp.rise[0]==='+'?1:-1]));
    this.search=null;
    // A Forts map's teams, bases, forcefield and bacon (see `fortress`); null for any other map.
    this.fort=fortress(map);
  }
  cellIndex(x,z){
    const i=Math.min(this.cols-1,Math.max(0,Math.floor((x-this.x0)/this.cell))),j=Math.min(this.rows-1,Math.max(0,Math.floor((z-this.z0)/this.cell)));
    return j*this.cols+i;
  }
  cellCenter(id){return [this.x0+(id%this.cols+.5)*this.cell,this.z0+(Math.floor(id/this.cols)+.5)*this.cell]}
  /** Navigation test used by zombies and routes: on the map and not in a blocked cell. */
  clear(x,z){return x>=this.x0&&x<this.x1&&z>=this.z0&&z<this.z1&&!this.blocked[this.cellIndex(x,z)]}
  /** Height of the walkable surface above the counter; the highest deck or ramp under the point wins (a round deck only inside its ellipse). In a sink the counter is the sink's floor, below 0. */
  height(x,z){
    let h=0;const d=this.decks,r=this.ramps,s=this.sinks;
    for(let i=0;i<s.length;i+=5)if(x>=s[i]&&x<=s[i+2]&&z>=s[i+1]&&z<=s[i+3])h=-s[i+4];
    for(let i=0;i<d.length;i+=6)if(d[i+4]>h&&x>=d[i]&&x<=d[i+2]&&z>=d[i+1]&&z<=d[i+3]&&(!d[i+5]||round(d.subarray(i,i+4),x,z)))h=d[i+4];
    for(let i=0;i<r.length;i+=8)if(x>=r[i]&&x<=r[i+2]&&z>=r[i+1]&&z<=r[i+3]){
      const t=r[i+6]?(z-r[i+1])/(r[i+3]-r[i+1]):(x-r[i])/(r[i+2]-r[i]),v=r[i+4]+(r[i+7]>0?t:1-t)*(r[i+5]-r[i+4]);
      if(v>h)h=v;
    }
    return h;
  }
  ground(x,z){return FLOOR+this.height(x,z)}
  /** Whether a hotdog whose feet are at height y may occupy the point: on the map and beside or above every block. */
  walkable(x,y,z){
    if(!(x>=this.x0&&x<this.x1&&z>=this.z0&&z<this.z1))return false;
    const w=this.walls;
    for(let i=0;i<w.length;i+=5)if(y<w[i+4]&&x>=w[i]&&x<=w[i+2]&&z>=w[i+1]&&z<=w[i+3])return false;
    return true;
  }
  /** One movement step from a to b, both [x, y, z]: ledges above STEP need a ramp or a jump. */
  canStep(a,b){const h=this.ground(b[0],b[2]);return h<=a[1]+STEP&&this.walkable(b[0],Math.max(h,b[1]),b[2])}
  /**
   * Distance a shot from o along the unit vector d travels before a block stops it, or the side or floor of a sink it has gone down into. (The counter top
   * itself is not cover: whatever fires decides where a shot meets it, with `sunk`.)
   */
  cover(o,d){
    let near=RANGE;const s=this.solids,k=this.sinks;
    for(let i=0;i<s.length;i+=5){const t=rayBox(o[0],o[1],o[2],d[0],d[1],d[2],s[i],FLOOR,s[i+1],s[i+2],s[i+4],s[i+3]);if(t<near)near=t}
    for(let i=0;i<k.length;i+=5){const t=basin(o,d,k,i);if(t<near)near=t}
    return near;
  }
  /** Whether the map has a sink (when it has none, nothing is below the counter top). */
  sunken(){return this.sinks.length>0}
  /** Whether [x, z] lies over a sink, where the counter top is open. */
  sunk(x,z){const s=this.sinks;for(let i=0;i<s.length;i+=5)if(x>=s[i]&&x<=s[i+2]&&z>=s[i+1]&&z<=s[i+3])return true;return false}
  line(a,b){
    const n=Math.max(1,Math.ceil(Math.hypot(b[0]-a[0],b[1]-a[1])/(this.cell*.35)));
    for(let i=0;i<=n;i++){const t=i/n;if(!this.clear(a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t))return false}
    return true;
  }
  /** Shortest clear route between two [x, z] points as waypoints ending at the goal; empty when there is none. */
  path(start,goal){
    if(!this.clear(start[0],start[1])||!this.clear(goal[0],goal[1]))return [];
    if(this.line(start,goal))return [[goal[0],goal[1]]];
    const {cols,rows,blocked}=this,cells=cols*rows;
    // Stamps mark which entries belong to this search, so nothing is cleared between searches.
    const s=this.search??=({cost:new Float64Array(cells),parent:new Int32Array(cells),stamp:new Uint32Array(cells),generation:0,score:new Float64Array(256),id:new Int32Array(256)});
    if(++s.generation===0xffffffff){s.stamp.fill(0);s.generation=1}
    const {cost,parent,stamp,generation}=s,first=this.cellIndex(start[0],start[1]),last=this.cellIndex(goal[0],goal[1]),gi=last%cols,gj=(last-gi)/cols;
    let size=0;
    const push=(score,id)=>{
      if(size===s.score.length){const score2=new Float64Array(size*2),id2=new Int32Array(size*2);score2.set(s.score);id2.set(s.id);s.score=score2;s.id=id2}
      const scores=s.score,ids=s.id;let i=size++;
      while(i>0){const p=(i-1)>>1;if(scores[p]<=score)break;scores[i]=scores[p];ids[i]=ids[p];i=p}
      scores[i]=score;ids[i]=id;
    };
    const pop=()=>{
      const scores=s.score,ids=s.id,top=ids[0],score=scores[--size],id=ids[size];let i=0;
      for(;;){let c=i*2+1;if(c>=size)break;if(c+1<size&&scores[c+1]<scores[c])c++;if(score<=scores[c])break;scores[i]=scores[c];ids[i]=ids[c];i=c}
      scores[i]=score;ids[i]=id;return top;
    };
    cost[first]=0;stamp[first]=generation;parent[first]=-1;push(0,first);
    while(size){
      const id=pop();
      if(id===last){
        const route=[[goal[0],goal[1]]];
        for(let at=last;at!==first;at=parent[at])route.push(this.cellCenter(at));
        route.reverse();
        // String pulling: drop every waypoint that the previous kept point can already see past.
        let previous=start;
        for(let i=0;i<route.length;i++){let j=i;while(j+1<route.length&&this.line(previous,route[j+1]))j++;route.splice(i,j-i);previous=route[i]}
        return route;
      }
      const x=id%cols,z=(id-x)/cols,base=cost[id];
      for(let dz=-1;dz<=1;dz++){
        const nz=z+dz;if(nz<0||nz>=rows)continue;
        for(let dx=-1;dx<=1;dx++){
          const nx=x+dx;if((!dx&&!dz)||nx<0||nx>=cols)continue;
          const next=nz*cols+nx;if(blocked[next])continue;
          // A diagonal move may not squeeze between two blocked corners.
          if(dx&&dz&&(blocked[z*cols+nx]||blocked[nz*cols+x]))continue;
          const c=base+(dx&&dz?DIAGONAL:1);
          if(stamp[next]===generation&&c>=cost[next])continue;
          cost[next]=c;stamp[next]=generation;parent[next]=id;
          const ax=Math.abs(nx-gi),az=Math.abs(nz-gj);
          push(c+Math.max(ax,az)+(DIAGONAL-1)*Math.min(ax,az),next);
        }
      }
    }
    return [];
  }
  /**
   * Where a walker at `old` should head to reach a `target` standing higher up: the foot of the cheapest ramp,
   * then a point just past its top. Null when the target is on the same level or no ramp leads up from here.
   */
  climbGoal(old,target){
    const own=old[1]-FLOOR,wanted=target[1]-FLOOR,r=this.ramps,x=old[0],z=old[2];
    let best=null,bestCost=Infinity;
    for(let i=0;i<r.length;i+=8){
      const base=r[i+4],top=r[i+5],alongZ=r[i+6]===1,up=r[i+7];
      const middle=alongZ?(r[i]+r[i+2])/2:(r[i+1]+r[i+3])/2,low=alongZ?(up>0?r[i+1]:r[i+3]):(up>0?r[i]:r[i+2]),high=alongZ?(up>0?r[i+3]:r[i+1]):(up>0?r[i+2]:r[i]);
      const head=alongZ?[middle,high+up*HEAD]:[high+up*HEAD,middle];
      // Already on a ramp that leads towards the target's level: keep climbing it rather than cutting off its side.
      if(own>base+.01&&top>own+.02&&wanted>=top-.05&&x>=r[i]-.02&&x<=r[i+2]+.02&&z>=r[i+1]-.02&&z<=r[i+3]+.02)return [head[0],this.ground(head[0],head[1]),head[1]];
      if(wanted<=own+CLIMB||base>own+.05||top<=own+.05)continue;
      const foot=alongZ?[middle,low+up*FOOT]:[low+up*FOOT,middle],reach=Math.hypot(foot[0]-x,foot[1]-z),after=Math.hypot(target[0]-head[0],target[2]-head[1]);
      // A board down into a sink is narrow and its foot is open on every side: a walker beside it, not yet in front of its foot, keeps making for the foot
      // rather than for the top up the board's side.
      const beside=base<0&&(alongZ?Math.abs(x-middle)>(r[i+2]-r[i])/2-.01:Math.abs(z-middle)>(r[i+3]-r[i+1])/2-.01);
      const atFoot=reach<.1&&!beside,goal=atFoot?head:foot,cost=(atFoot?Math.hypot(head[0]-x,head[1]-z):reach+Math.abs(high-low))+after;
      if(cost<bestCost){bestCost=cost;best=goal}
    }
    return best&&[best[0],this.ground(best[0],best[1]),best[1]];
  }
  /** Players share the starts in order, wrapping when a map has fewer starts than players. */
  startPosition(player){const [x,z]=this.map.starts[player%this.map.starts.length].p;return [x,this.ground(x,z),z]}
  startYaw(player){return this.map.starts[player%this.map.starts.length].yaw}
  /** Zombies walk the counter itself, so they always appear at floor height (a sink's floor, in a sink). */
  spawnPosition(zombie){const [x,z]=this.map.spawns[zombie%this.map.spawns.length].p;return [x,FLOOR+Math.min(this.height(x,z),0),z]}
  pickupPosition(index){const [x,z]=this.map.pickups[index].p;return [x,this.ground(x,z)+.04,z]}
  /** Forts: where the `n`th hotdog of a team starts ([x, y, z]) and the way it faces; a team shares its starts in order. */
  teamStart(team,n){const own=this.fort.starts[team],{p:[x,z],yaw}=own[n%own.length];return {p:[x,this.ground(x,z),z],yaw}}
  /** Forts: where the bacon lies, on the surface at its spot. */
  baconPosition(){const [x,z]=this.fort.bacon;return [x,this.ground(x,z),z]}
  /** Forts: whether [x, z] is inside a team's base. */
  inBase(team,x,z){const r=this.fort.bases[team];return x>=r[0]&&x<=r[2]&&z>=r[1]&&z<=r[3]}
  /** Forts: the side of the forcefield a point is on, -1 or 1 (a team's is `fort.sides[team]`). */
  side(x,z){return (this.fort.axis?z:x)<this.fort.mid?-1:1}
}
export const compile=map=>new Level(map);
/** Whether [x, z] lies on a round deck over `rect` ([minX, minZ, maxX, maxZ]): inside the ellipse that fills it. map.rs `round` works it out in the same order. */
export function round(rect,x,z){const dx=(x-(rect[0]+rect[2])/2)/((rect[2]-rect[0])/2),dz=(z-(rect[1]+rect[3])/2)/((rect[3]-rect[1])/2);return dx*dx+dz*dz<=1}
/**
 * A Forts map's level while its forcefield is up: the field stands as one more block, the last, of the greatest height a block may have,
 * so that bodies, routes, shots and loose objects are all stopped by it. Its `map` is the map itself, without that block.
 */
export function fielded(map){
  const fort=fortress(map),level=new Level({...map,blocks:[...map.blocks,{rect:fort.field,height:LIMITS.height,style:'steel'}]});
  level.map=map;level.fort=fort;return level;
}

const ISSUES={
  'start-blocked':['error',(n)=>`Start ${n} is inside a block or off the map, so that hotdog could not move`],
  'spawn-blocked':['error',(n)=>`Zombie spawn ${n} is inside a block's clearance or off the map, so its zombies could not move`],
  'few-starts':['warning',(n,level)=>`Only ${level.map.starts.length} of ${PLAYERS} starts are set; hotdogs will share them`],
  'pickup-blocked':['warning',(n)=>`Pickup ${n} is inside a block or off the map`],
  'pickup-unreachable':['warning',(n)=>`Pickup ${n} cannot be reached on foot from any start`],
  'deck-unreachable':['warning',(n)=>`Deck ${n} has no way up: add a ramp or lower it`],
  'spawn-isolated':['warning',(n)=>`Zombies from spawn ${n} cannot reach any start`],
  'ramp-steep':['warning',(n)=>`Ramp ${n} rises more than its length; climbing it depends on frame rate`],
  'sink-trap':['warning',(n)=>`Sink ${n} has no ramp out: a hotdog that drops in needs a stimmed jump to leave, and zombies and bots cannot leave at all`],
  'outside':['warning',(n,level,kind)=>`${kind[0].toUpperCase()+kind.slice(1,-1)} ${n} lies entirely off the map`],
  'beyond-room':['warning',()=>`The counter reaches past the largest kitchen the game builds, which holds a counter from x = ${ROOM[0]} to ${ROOM[1]} and up to z = ${ROOM[2]}: what lies beyond its walls cannot be seen`],
  // A Forts map (`team` is the team's name where one is meant).
  'field-missing':['error',()=>'A Forts map needs a forcefield: add a field zone right across the map, between the bases'],
  'bacon-missing':['error',()=>'A Forts map needs the bacon: add a bacon zone in the middle'],
  'base-missing':['error',(n,level,kind,team)=>`The ${team} have no base: add a base zone for them`],
  'zone-extra':['warning',(n)=>`Zone ${n} is a second one of its kind and is ignored`],
  'bases-same-side':['error',()=>'Both bases are on the same side of the forcefield'],
  'base-in-field':['error',(n)=>`Zone ${n}, a base, lies across the forcefield`],
  'field-open':['error',(n)=>`The forcefield (zone ${n}) does not close the map: hotdogs can walk round it during the build`],
  'start-teamless':['error',(n)=>`Start ${n} belongs to no team`],
  'start-wrong-side':['error',(n,level,kind,team)=>`Start ${n} belongs to the ${team} but is on the other side of the forcefield`],
  'team-no-start':['error',(n,level,kind,team)=>`The ${team} have no start`],
  'team-few-starts':['warning',(n,level,kind,team)=>`The ${team} have fewer than ${FORT_PLAYERS/2} starts; their hotdogs will share them`],
  'bacon-blocked':['error',(n)=>`The bacon (zone ${n}) is inside a block or off the map`],
  'bacon-unreachable':['error',(n,level,kind,team)=>`The ${team} cannot reach the bacon on foot once the forcefield is down`],
  'base-unreachable':['error',(n,level,kind,team)=>`The ${team} cannot carry the bacon home: no way on foot from the bacon to their base`],
  'stock-missing':['warning',(n,level,kind,team)=>`The ${team} have no loose objects on their side to build with`],
  'zones-partial':['warning',()=>'Some Forts zones are set but not the whole layout (a base for each team, a field and the bacon): the map cannot be played as Forts until they all are'],
};
const ORDER=['starts','spawns','pickups','blocks','decks','ramps','sinks','props','zones'];

/**
 * Per-cell facts for the editor's overlays and the list of problems that would spoil a round:
 * groundH (surface height above the counter), stand (a hotdog fits), reach (walkable from a start),
 * zdist (metres a zombie travels from the nearest spawn, -1 when it cannot arrive).
 */
export function analyze(level){
  const {cols,rows,cell,map,x0,z0,blocked}=level,cells=cols*rows;
  const groundH=new Float32Array(cells),blockTop=new Float32Array(cells),rampOf=new Int32Array(cells).fill(-1),stand=new Uint8Array(cells),reach=new Uint8Array(cells),zdist=new Float32Array(cells).fill(-1);
  // Cells whose centres fall inside a rectangle.
  const cover=(r,grow,visit)=>{
    const i0=Math.max(0,Math.ceil((r[0]-grow-x0)/cell-.5)),i1=Math.min(cols-1,Math.floor((r[2]+grow-x0)/cell-.5)),j0=Math.max(0,Math.ceil((r[1]-grow-z0)/cell-.5)),j1=Math.min(rows-1,Math.floor((r[3]+grow-z0)/cell-.5));
    for(let j=j0;j<=j1;j++)for(let i=i0;i<=i1;i++)visit(j*cols+i,i,j);
    return i0<=i1&&j0<=j1;
  };
  // A sink's floor is below the counter: everything else stands higher.
  for(const sink of map.sinks??[])cover(sink.rect,0,id=>{groundH[id]=-sink.depth});
  const onDeck=(deck,id)=>deck.style!==ROUND||round(deck.rect,...level.cellCenter(id));
  for(const deck of map.decks)cover(deck.rect,0,id=>{if(deck.height>groundH[id]&&onDeck(deck,id)){groundH[id]=deck.height;rampOf[id]=-1}});
  map.ramps.forEach((ramp,index)=>{
    const [rx0,rz0,rx1,rz1]=ramp.rect,alongZ=ramp.rise[1]==='z',up=ramp.rise[0]==='+';
    cover(ramp.rect,0,(id,i,j)=>{
      const t=alongZ?(z0+(j+.5)*cell-rz0)/(rz1-rz0):(x0+(i+.5)*cell-rx0)/(rx1-rx0),h=ramp.base+(up?t:1-t)*(ramp.height-ramp.base);
      if(h>groundH[id]){groundH[id]=h;rampOf[id]=index}
    });
  });
  for(const block of map.blocks)cover(block.rect,MARGIN,id=>{if(block.height+LIP>blockTop[id])blockTop[id]=block.height+LIP});
  let standable=0;
  // (Where no block stands, blockTop is 0, and a sink's floor is below it.)
  for(let id=0;id<cells;id++)if(groundH[id]>=blockTop[id]||!blockTop[id]){stand[id]=1;standable++}

  const issues=[],report=(code,kind,index,team)=>issues.push({severity:ISSUES[code][0],code,kind,index,message:ISSUES[code][1](index+1,level,kind,team)});
  const inside=(x,z)=>x>=level.x0&&x<level.x1&&z>=level.z0&&z<level.z1;

  // Walking: four-connected flood from every start. A step may rise by STEP, or follow a ramp it is already on.
  const queue=new Int32Array(cells);let head=0,tail=0,reachable=0;
  map.starts.forEach((start,index)=>{
    const [x,z]=start.p;
    if(!level.walkable(x,level.ground(x,z),z)){report('start-blocked','starts',index);return}
    const id=level.cellIndex(x,z);if(stand[id]&&!reach[id]){reach[id]=1;queue[tail++]=id}
  });
  while(head<tail){
    const id=queue[head++],i=id%cols,h=groundH[id],ramp=rampOf[id];reachable++;
    for(let k=0;k<4;k++){
      const next=k===0?(i>0?id-1:-1):k===1?(i<cols-1?id+1:-1):k===2?id-cols:id+cols;
      if(next<0||next>=cells||reach[next]||!stand[next])continue;
      if(groundH[next]>h+STEP&&!(ramp>=0&&rampOf[next]===ramp))continue;
      reach[next]=1;queue[tail++]=next;
    }
  }

  // Zombies: connected components of clear cells, then travel distance from the spawns with 5/7 chamfer weights.
  const component=new Int32Array(cells).fill(-1);let components=0;
  const neighbours=(id,visit)=>{
    const i=id%cols,j=(id-i)/cols;
    for(let dj=-1;dj<=1;dj++){const nj=j+dj;if(nj<0||nj>=rows)continue;
      for(let di=-1;di<=1;di++){const ni=i+di;if((!di&&!dj)||ni<0||ni>=cols)continue;
        const next=nj*cols+ni;if(blocked[next]||(di&&dj&&(blocked[j*cols+ni]||blocked[nj*cols+i])))continue;
        visit(next,di&&dj?7:5);
      }}
  };
  for(let seed=0;seed<cells;seed++){
    if(blocked[seed]||component[seed]>=0)continue;
    head=tail=0;component[seed]=components;queue[tail++]=seed;
    while(head<tail)neighbours(queue[head++],next=>{if(component[next]<0){component[next]=components;queue[tail++]=next}});
    components++;
  }
  const startComponents=new Set(map.starts.filter(s=>inside(...s.p)).map(s=>component[level.cellIndex(...s.p)]).filter(c=>c>=0));
  const far=0x3fffffff,steps=new Int32Array(cells).fill(far),buckets=Array.from({length:8},()=>[]);let pending=0;
  map.spawns.forEach((spawn,index)=>{
    const [x,z]=spawn.p;
    if(!level.clear(x,z)){report('spawn-blocked','spawns',index);return}
    const id=level.cellIndex(x,z);
    if(!startComponents.has(component[id]))report('spawn-isolated','spawns',index);
    if(steps[id]){steps[id]=0;buckets[0].push(id);pending++}
  });
  // Dial's algorithm: edge weights are at most 7, so eight rotating buckets order the frontier without a heap.
  for(let current=0;pending;current++){
    const bucket=buckets[current&7];
    while(bucket.length){
      const id=bucket.pop();pending--;
      if(steps[id]!==current)continue;
      neighbours(id,(next,weight)=>{const d=current+weight;if(d<steps[next]){steps[next]=d;buckets[d&7].push(next);pending++}});
    }
  }
  for(let id=0;id<cells;id++)if(steps[id]!==far)zdist[id]=steps[id]*cell/5;

  // (Every map but the dojo can be played as a siege or Free For All, which have five hotdogs.)
  if(map.starts.length<PLAYERS&&map.mode!=='kungfu')report('few-starts','starts',0);
  map.pickups.forEach((pickup,index)=>{
    const [x,z]=pickup.p;
    if(!level.walkable(x,level.ground(x,z),z))report('pickup-blocked','pickups',index);
    else if(!reach[level.cellIndex(x,z)])report('pickup-unreachable','pickups',index);
  });
  map.decks.forEach((deck,index)=>{
    let any=false,reached=false;
    cover(deck.rect,0,id=>{if(onDeck(deck,id)&&stand[id]&&Math.abs(groundH[id]-deck.height)<1e-6){any=true;reached||=!!reach[id]}});
    if(any&&!reached)report('deck-unreachable','decks',index);
  });
  map.ramps.forEach((ramp,index)=>{
    const run=ramp.rise[1]==='z'?ramp.rect[3]-ramp.rect[1]:ramp.rect[2]-ramp.rect[0];
    if(ramp.height-ramp.base>run)report('ramp-steep','ramps',index);
  });
  // A sink needs a way out: a ramp whose foot stands on its floor and whose top reaches the counter.
  (map.sinks??[]).forEach((sink,index)=>{
    const [x0,z0,x1,z1]=sink.rect,out=map.ramps.some(ramp=>{
      const [rx0,rz0,rx1,rz1]=ramp.rect,foot=ramp.rise==='+x'?[rx0,(rz0+rz1)/2]:ramp.rise==='-x'?[rx1,(rz0+rz1)/2]:ramp.rise==='+z'?[(rx0+rx1)/2,rz0]:[(rx0+rx1)/2,rz1];
      return ramp.base<=-sink.depth+STEP&&ramp.height>=0&&foot[0]>=x0&&foot[0]<=x1&&foot[1]>=z0&&foot[1]<=z1;
    });
    if(!out)report('sink-trap','sinks',index);
  });
  // Forts, on a map that can be played so (made for it, or with the whole layout): both bases, on opposite sides of a forcefield that closes the map; every
  // start on its team's side; the bacon within reach of both bases, and each base within reach of the bacon, once the field is down; something to build
  // with on each side. An index of -1 means something the map lacks. (A map made for another mode whose starts name no team has them sorted by side.)
  if(fortress(map)){
    const fort=level.fort,zones=map.zones,found=(kind,team)=>zones.flatMap((z,i)=>z.kind===kind&&(team===undefined||z.team===team)?[i]:[]);
    const fields=found('field'),bacons=found('bacon'),bases=TEAMS.map(team=>found('base',team)),middle=r=>[(r[0]+r[2])/2,(r[1]+r[3])/2];
    if(!fields.length)report('field-missing','zones',-1);
    if(!bacons.length)report('bacon-missing','zones',-1);
    bases.forEach((own,t)=>{if(!own.length)report('base-missing','zones',-1,TEAM_NAMES[t])});
    for(const extra of [...fields.slice(1),...bacons.slice(1),...bases.flatMap(own=>own.slice(1))].sort((a,b)=>a-b))report('zone-extra','zones',extra);
    if(bases[0].length&&bases[1].length&&level.side(...middle(zones[bases[0][0]].rect))===level.side(...middle(zones[bases[1][0]].rect)))report('bases-same-side','zones',bases[1][0]);
    const f=fort.field;
    bases.forEach(own=>{if(own.length){const r=zones[own[0]].rect;if(r[0]<f[2]&&r[2]>f[0]&&r[1]<f[3]&&r[3]>f[1])report('base-in-field','zones',own[0])}});
    const counts=[0,0];
    const named=map.mode==='forts'||map.starts.some(s=>s.team);
    map.starts.forEach((start,index)=>{
      const t=named?TEAMS.indexOf(start.team):fort.sides.indexOf(level.side(...start.p));
      if(t<0){report('start-teamless','starts',index);return}
      counts[t]++;if(level.side(...start.p)!==fort.sides[t])report('start-wrong-side','starts',index,TEAM_NAMES[t]);
    });
    counts.forEach((count,t)=>{if(!count)report('team-no-start','starts',-1,TEAM_NAMES[t]);else if(count<FORT_PLAYERS/2)report('team-few-starts','starts',-1,TEAM_NAMES[t])});
    // The same walk as from the starts, from any cells, keeping out of `barred` ones (the forcefield while it stands).
    const walk=(seeds,barred)=>{
      const seen=new Uint8Array(cells);let from=0,to=0;
      for(const id of seeds)if(stand[id]&&!seen[id]&&!(barred&&barred[id])){seen[id]=1;queue[to++]=id}
      while(from<to){
        const id=queue[from++],i=id%cols,h=groundH[id],ramp=rampOf[id];
        for(let k=0;k<4;k++){
          const next=k===0?(i>0?id-1:-1):k===1?(i<cols-1?id+1:-1):k===2?id-cols:id+cols;
          if(next<0||next>=cells||seen[next]||!stand[next]||(barred&&barred[next]))continue;
          if(groundH[next]>h+STEP&&!(ramp>=0&&rampOf[next]===ramp))continue;
          seen[next]=1;queue[to++]=next;
        }
      }
      return seen;
    };
    const homes=fort.starts.map((own,t)=>[...own.map(s=>s.p),middle(fort.bases[t])].filter(p=>inside(...p)).map(p=>level.cellIndex(...p)));
    // (A start on the wrong side, or two bases on one, is reported as that and is not the field's doing.)
    if(fields.length&&fort.sides[0]!==fort.sides[1]){
      const barred=new Uint8Array(cells);cover(f,MARGIN,id=>{barred[id]=1});
      const own=t=>homes[t].filter(id=>level.side(...level.cellCenter(id))===fort.sides[t]),walled=walk(own(0),barred);
      if(own(1).some(id=>walled[id]))report('field-open','zones',fields[0]);
    }
    const spot=inside(...fort.bacon)?level.cellIndex(...fort.bacon):-1,which=bacons.length?bacons[0]:-1;
    if(spot<0||!stand[spot]){if(bacons.length)report('bacon-blocked','zones',which)}
    else{
      const back=walk([spot]);
      TEAMS.forEach((team,t)=>{
        if(!walk(homes[t])[spot])report('bacon-unreachable','zones',which,TEAM_NAMES[t]);
        else{let home=false;cover(fort.bases[t],0,id=>{home||=!!back[id]});if(!home)report('base-unreachable','zones',bases[t].length?bases[t][0]:-1,TEAM_NAMES[t])}
      });
    }
    TEAMS.forEach((team,t)=>{if(!map.props.some(prop=>LOOSE.includes(prop.kind)&&level.side(...prop.p)===fort.sides[t]))report('stock-missing','props',-1,TEAM_NAMES[t])});
  }else if(map.zones.length)report('zones-partial','zones',0);
  // (A bin stands on the kitchen floor beside the counter, off the map by design.)
  for(const kind of ['blocks','decks','ramps','sinks','props','zones'])(map[kind]??[]).forEach((item,index)=>{
    if(item.kind==='bin')return;
    const r=item.rect??[item.p[0],item.p[1],item.p[0],item.p[1]];
    if(r[2]<level.x0||r[0]>level.x1||r[3]<level.z0||r[1]>level.z1)report('outside',kind,index);
  });
  if(level.x0<ROOM[0]||level.x1>ROOM[1]||level.z1>ROOM[2])report('beyond-room','bounds',-1);
  issues.sort((a,b)=>(a.severity==='error'?0:1)-(b.severity==='error'?0:1)||ORDER.indexOf(a.kind)-ORDER.indexOf(b.kind)||a.index-b.index);
  let blockedCells=0;for(let id=0;id<cells;id++)blockedCells+=blocked[id];
  const stats={cells,cols,rows,blocked:blockedCells,standable,reachable,components,starts:map.starts.length,spawns:map.spawns.length,pickups:map.pickups.length,blocks:map.blocks.length,decks:map.decks.length,ramps:map.ramps.length,sinks:(map.sinks??[]).length,props:map.props.length,zones:map.zones.length};
  return {groundH,blockTop,rampOf,stand,reach,zdist,issues,stats};
}

// FNV-1a over little-endian 32-bit words; the Rust tests compute the same fingerprints.
const fnv=words=>{let h=0x811c9dc5;for(const word of words)for(let shift=0;shift<32;shift+=8){h^=word>>>shift&255;h=Math.imul(h,0x01000193)}return (h>>>0).toString(16).padStart(8,'0')};
/** Fingerprint of the navigation grid's size and blocked cells. */
export function gridHash(level){return fnv([level.cols,level.rows,...level.blocked])}
/** Fingerprint of the surface height at every cell centre, in tenths of a millimetre. */
export function groundHash(level){
  const words=[];for(let id=0;id<level.cols*level.rows;id++){const [x,z]=level.cellCenter(id);words.push(quantum(level.height(x,z)))}
  return fnv(words);
}
