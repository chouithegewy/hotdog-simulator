// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Editor document model: the open map, editing operations, undo history, selection, picking and snapping.
// No DOM and no I/O, so the same file runs in the browser and under `node --test editor/model.test.mjs`.
import {FORMAT,VERSION,KINDS,LIMITS,TEAMS,AMMO_BOX,MapError,normalize,normalizeItem,normalizeSettings,checkCount,stringify,blank,compile,analyze,fortress} from '../tools/network/map.mjs';

/** Smallest side of a drawn rectangle, the plan radius of a point marker, and how many undo steps are kept. */
export const MIN=.01,MARKER=.03,HISTORY=400;
export const LABELS={starts:'Start',spawns:'Spawn',pickups:'Pickup',blocks:'Block',decks:'Deck',ramps:'Ramp',sinks:'Sink',props:'Prop',zones:'Zone'};
// Higher layers are drawn later and picked first. A Forts map's areas (the bases and the forcefield) lie under everything, and a sink, a hole in the counter, under
// everything that stands on it.
const LAYER={zones:-1,sinks:-.5,decks:0,ramps:1,blocks:2,props:3,starts:4,spawns:4,pickups:4};
export const layerOf=(kind,item)=>item.rect?LAYER[kind]:4;
// Half the width and depth of each loose object on the plan, in metres, as tools/network/presets.mjs shapes them.
export const FOOTPRINT={bowl:[.155,.155],crate:[.05,.05],pillow:[.07,.05],napkin:[.225,.225],chair:[.045,.045],tray:[.15,.1],cereal:[.115,.04],cup:[.067,.04],plate:[.127,.127],tin:[.0355,.0355],board:[.225,.15],
  dish:[.0715,.0715],roll:[.058,.058],salt:[.0215,.0215],pepper:[.0215,.0215],breadbox:[.2,.125]};
// The same for the props that are not loose and are bigger than a marker: a bin, which stands on the kitchen floor, 0.34 m wide and 0.3 m deep (src/kitchen.rs).
export const FIXED={bin:[.17,.15]};
const footprintOf=item=>FOOTPRINT[item.kind]??FIXED[item.kind];
/** What each block style among KITS is called in the panel and on the plan. */
export const KIT_NAME={microwave:'Microwave',toaster:'Toaster',kettle:'Kettle','knife-block':'Knife block',canister:'Canister',jar:'Jar',cereal:'Cereal box',
  'paper-towel':'Paper towels','dish-rack':'Dish rack','spice-rack':'Spice rack','fruit-bowl':'Fruit bowl',faucet:'Tap',griddle:'Electric griddle',gong:'Wok gong'};
/** The sink whose basin holds the point [x, z] on the plan, or null. */
export const sinkAt=(map,x,z)=>(map.sinks??[]).find(({rect:r})=>x>=r[0]&&x<=r[2]&&z>=r[1]&&z<=r[3])??null;
/// How far a point object reaches from its position: loose objects are bigger than a marker.
export const reach=(item,marker=0)=>Math.max(marker,...(footprintOf(item)??[0]));
/** Half the plan width and depth, along x and z, of the box round a point object: a loose object's footprint turned by its yaw, never less than the marker. */
export function extent(item,marker=0){
  const footprint=footprintOf(item);
  if(!footprint)return [marker,marker];
  const c=Math.abs(Math.cos(item.yaw??0)),s=Math.abs(Math.sin(item.yaw??0)),[hx,hz]=footprint;
  return [Math.max(marker,hx*c+hz*s),Math.max(marker,hx*s+hz*c)];
}
/** Rounds a new gun pickup holds, by weapon number (a full load); a box of ammunition (AMMO_BOX, 0) has no count. */
export const ROUNDS=[null,12,6,90,4,40,60,150];
/**
 * The fields of a pickup that holds `weapon`: a box of ammunition is {weapon: 0} (normalizing drops any `ammo` it had),
 * a gun {weapon, ammo} with that gun's full load.
 */
export const holding=weapon=>weapon===AMMO_BOX?{weapon}:{weapon,ammo:ROUNDS[weapon]??10};

const UNIT=1e4,trim=v=>Math.floor(v*UNIT+.5)/UNIT;
const same=(a,b)=>{
  if(a===b)return true;
  if(Array.isArray(a))return Array.isArray(b)&&a.length===b.length&&a.every((v,i)=>same(v,b[i]));
  if(!a||!b||typeof a!=='object'||typeof b!=='object')return false;
  const keys=Object.keys(a);
  return keys.length===Object.keys(b).length&&keys.every(key=>same(a[key],b[key]));
};
const sameMap=(a,b)=>a===b||(a.name===b.name&&a.theme===b.theme&&a.mode===b.mode&&a.build===b.build&&same(a.respawn,b.respawn)&&!!a.cabinets===!!b.cabinets&&same(a.bounds,b.bounds)&&same(a.nav,b.nav)&&KINDS.every(kind=>same(a[kind],b[kind])));
const refuse=error=>{if(error instanceof MapError)return {ok:false,error:error.message};throw error};
const label=(kind,index)=>`${LABELS[kind]} ${index+1}`;
// "Ramp 1.height must be…" reads better as "Ramp 1 height must be…".
const tidy=message=>message.replace(/^([A-Z][a-z]+ \d+)\.(\w+)/,'$1 $2');
const offset=(item,dx,dz)=>item.rect
  ?{...item,rect:[item.rect[0]+dx,item.rect[1]+dz,item.rect[2]+dx,item.rect[3]+dz]}
  :{...item,p:[item.p[0]+dx,item.p[1]+dz]};
const bare=({id,...rest})=>rest;

/** The plan rectangle an object occupies; a point object is the box round it (`extent`), at least a square of the given radius around its position. */
export function itemBox(item,marker=0){
  if(item.rect)return item.rect;
  const [rx,rz]=extent(item,marker);
  return [item.p[0]-rx,item.p[1]-rz,item.p[0]+rx,item.p[1]+rz];
}
/** The rectangle around a list of objects, or null when the list is empty. */
export function boxOf(items,marker=0){
  let box=null;
  for(const item of items){
    const b=itemBox(item,marker);
    box=box?[Math.min(box[0],b[0]),Math.min(box[1],b[1]),Math.max(box[2],b[2]),Math.max(box[3],b[3])]:b.slice();
  }
  return box;
}
/** A rectangle from the two corners of a drag, grown away from the first corner if the drag was too short. */
export function rectFromDrag(a,b,min=MIN){
  const axis=i=>b[i]<a[i]?[Math.min(b[i],a[i]-min),a[i]]:[a[i],Math.max(b[i],a[i]+min)];
  const [x0,x1]=axis(0),[z0,z1]=axis(1);
  return [x0,z0,x1,z1].map(trim);
}
/** Moves the edges named by a handle ({x, z} each -1, 0 or 1) to a point; an edge never passes its opposite. */
export function resizeRect(rect,handle,x,z,min=MIN){
  let [x0,z0,x1,z1]=rect;
  if(handle.x<0)x0=Math.min(x,x1-min);else if(handle.x>0)x1=Math.max(x,x0+min);
  if(handle.z<0)z0=Math.min(z,z1-min);else if(handle.z>0)z1=Math.max(z,z0+min);
  return [x0,z0,x1,z1].map(trim);
}
/** The uphill direction for a ramp drawn from a to b: along the longer side, towards where the drag ended. */
export function riseFromDrag(a,b){
  const dx=b[0]-a[0],dz=b[1]-a[1];
  return Math.abs(dx)>=Math.abs(dz)?(dx>=0?'+x':'-x'):(dz>=0?'+z':'-z');
}
/** Rounds a point to multiples of step; a step of 0 only trims it to the 0.1 mm lattice. */
export function snapPoint(p,step){
  const stepU=Math.round(step*UNIT);
  return p.map(v=>stepU>0?Math.round(v*UNIT/stepU)*stepU/UNIT:trim(v));
}
/** Pulls a coordinate onto the nearest candidate within reach; guide is where to draw the alignment line. */
export function edgeSnap(value,candidates,reach){
  let best=null,distance=reach;
  for(const candidate of candidates){const d=Math.abs(candidate-value);if(d<=distance&&(best===null||d<distance)){best=candidate;distance=d}}
  return best===null?{value,guide:null}:{value:best,guide:best};
}
/** Returns the map with the listed objects moved flush to one edge (or the middle) of the box around them. */
export function alignItems(map,ids,edge){
  const set=new Set(ids),chosen=KINDS.flatMap(kind=>map[kind].filter(item=>set.has(item.id))),box=boxOf(chosen);
  if(chosen.length<2)return map;
  const z=edge.endsWith('Z'),lo=z?1:0,hi=lo+2,mode=edge.slice(0,3),target=mode==='min'?box[lo]:mode==='max'?box[hi]:(box[lo]+box[hi])/2;
  const out={...map};
  for(const kind of KINDS)if(map[kind].some(item=>set.has(item.id)))out[kind]=map[kind].map(item=>{
    if(!set.has(item.id))return item;
    const b=itemBox(item),d=trim(target-(mode==='min'?b[lo]:mode==='max'?b[hi]:(b[lo]+b[hi])/2));
    return d?offset(item,z?0:d,z?d:0):item;
  });
  return out;
}

/**
 * A map made a Forts map. One with no zones yet is given the two bases, the forcefield and the bacon where the game would
 * assume them (`fortress`); every start that names no team joins the team whose side it is on, and a team left without a
 * start gets one in the middle of its base, facing the field.
 */
export function fortified(map){
  const out={...map,mode:'forts'};
  let fort=fortress(out);
  if(!(map.zones??[]).length)out.zones=[...TEAMS.map((team,t)=>({kind:'base',team,rect:fort.bases[t]})),{kind:'field',rect:fort.field},{kind:'bacon',p:fort.bacon}];
  const side=p=>p[fort.axis]<fort.mid?-1:1;
  if(map.starts.some(start=>!start.team))out.starts=map.starts.map(start=>start.team?start:{...start,team:TEAMS[side(start.p)===fort.sides[0]?0:1]});
  fort=fortress(out);
  TEAMS.forEach((team,t)=>{if(!out.starts.some(start=>start.team===team))out.starts=[...out.starts,{...fort.starts[t][0],team}]});
  return out;
}

export class Doc {
  #map;#saved;#selection=[];#undo=[];#redo=[];#drag=null;#listeners=new Set;
  #next=1;#known=new WeakSet;#index=null;#level=null;#analysis=null;

  constructor(input){this.#map=this.#saved=this.#canonical(normalize(input??blank()));}

  get map(){return this.#map}
  get selection(){return this.#selection}
  get canUndo(){return this.#undo.length>0}
  get canRedo(){return this.#redo.length>0}
  /** Whether the map differs from what was last loaded or marked as saved. */
  get dirty(){return this.#map!==this.#saved}
  get dragging(){return this.#drag!==null}
  /** The map compiled for play; rebuilt only when the map changes. */
  get level(){return this.#level??=compile(this.#map)}
  /** Overlay data and the list of problems for the current map. */
  get analysis(){return this.#analysis??=analyze(this.level)}
  /** Reasons the game would refuse this map as a file, on top of what the analysis reports. */
  get overLimit(){
    const messages=[];
    for(const kind of KINDS)try{checkCount(kind,this.#map[kind].length)}catch(error){messages.push(error.message)}
    return messages;
  }

  /** Listens for {type: 'map' | 'selection', reason}; returns a function that stops listening. */
  on(listener){this.#listeners.add(listener);return ()=>this.#listeners.delete(listener)}
  #emit(type,reason){for(const listener of [...this.#listeners])listener({type,reason,doc:this})}

  /** Opens a map from text or an object. Throws MapError and keeps the open map when it is not valid. */
  load(input,{saved=true}={}){
    const map=this.#canonical(normalize(input));
    this.#drag=null;this.#undo=[];this.#redo=[];this.#saved=saved?map:null;
    this.#show(map,[],'load');
  }
  /** The map as canonical file text. */
  toJSON(){
    const out={...this.#map};
    for(const kind of KINDS)out[kind]=out[kind].map(bare);
    return stringify(out);
  }
  markSaved(){this.#saved=this.#map}

  *items(){for(const kind of KINDS){const list=this.#map[kind];for(let index=0;index<list.length;index++)yield {kind,index,item:list[index]}}}
  /** {kind, index, item} for an id, or null. */
  find(id){
    if(!this.#index){
      this.#index=new Map;
      KINDS.forEach((kind,k)=>{const list=this.#map[kind];for(let i=0;i<list.length;i++)this.#index.set(list[i].id,i*16+k)});
    }
    const code=this.#index.get(id);
    if(code===undefined)return null;
    const kind=KINDS[code&15],index=code>>>4;
    return {kind,index,item:this.#map[kind][index]};
  }

  // ---- canonical form -------------------------------------------------------------------------------------------
  // Every object the document holds has been checked by the map library and carries a unique id. Objects are never
  // mutated, so an unchanged object or list is shared between history steps and recognised without checking it again.
  #mint(kind,item,index,id=this.#next++){
    let clean;
    try{clean=normalizeItem(kind,item,label(kind,index))}catch(error){if(error instanceof MapError)error.message=tidy(error.message);throw error}
    const made={id,...clean};
    this.#known.add(made);
    return made;
  }
  #canonical(input,limits=false){
    if(this.#known.has(input))return input;
    if(!input||typeof input!=='object')throw new MapError('An edit must produce a map');
    const settings=normalizeSettings(input),now=this.#map;
    if(now&&same(settings.bounds,now.bounds))settings.bounds=now.bounds;
    if(now&&same(settings.nav,now.nav))settings.nav=now.nav;
    const map={format:FORMAT,version:VERSION,...settings},taken=new Set;
    let fresh=false;
    for(const kind of KINDS){
      const list=input[kind]??[];
      if(!Array.isArray(list))throw new MapError(`${kind} must be a list`);
      checkCount(kind,limits?list.length:Math.min(list.length,LIMITS[kind]));
      fresh||=!this.#known.has(list);
    }
    // Ids already in use, so a new or copied object can never share one.
    if(fresh)for(const kind of KINDS)for(const item of input[kind]??[])if(this.#known.has(item))taken.add(item.id);
    for(const kind of KINDS){
      const list=input[kind]??[];
      if(this.#known.has(list)){map[kind]=list;continue}
      const seen=new Set;
      map[kind]=list.map((item,index)=>{
        if(this.#known.has(item)&&!seen.has(item)){seen.add(item);return item}
        // A changed copy keeps the id of the object it replaces; anything else gets a new one.
        const keep=Number.isInteger(item?.id)&&item.id>0&&item.id<this.#next&&!taken.has(item.id);
        if(keep)taken.add(item.id);
        return keep?this.#mint(kind,item,index,item.id):this.#mint(kind,item,index);
      });
      this.#known.add(map[kind]);
    }
    this.#known.add(map);
    return map;
  }

  // ---- state changes --------------------------------------------------------------------------------------------
  #show(map,selection,reason){
    const changed=map!==this.#map;
    if(changed){this.#map=map;this.#index=this.#level=this.#analysis=null}
    const kept=selection.length?selection.filter(id=>this.find(id)):selection,moved=!same(kept,this.#selection);
    if(moved)this.#selection=kept;
    if(changed)this.#emit('map',reason);
    if(moved)this.#emit('selection',reason);
  }
  #record(before,next,selection,reason){
    this.#undo.push(before);
    if(this.#undo.length>HISTORY)this.#undo.shift();
    this.#redo=[];
    this.#show(next,selection,reason);
  }
  /**
   * Runs one edit: change(map) returns the new map, which is checked before it replaces the open one.
   * Returns {ok: true}, or {ok: false, error} leaving everything as it was. options.select picks the selection afterwards.
   */
  apply(change,options={}){
    this.cancel();
    let next;
    try{next=this.#canonical(change(this.#map),options.limits)}catch(error){return refuse(error)}
    const selection=options.select?options.select():this.#selection;
    if(sameMap(next,this.#map))this.#show(this.#map,selection,'edit');
    else this.#record({map:this.#map,selection:this.#selection},next,selection,'edit');
    return {ok:true};
  }
  undo(){
    this.cancel();
    const step=this.#undo.pop();
    if(!step)return false;
    this.#redo.push({map:this.#map,selection:this.#selection});
    this.#show(step.map,step.selection,'undo');
    return true;
  }
  redo(){
    this.cancel();
    const step=this.#redo.pop();
    if(!step)return false;
    this.#undo.push({map:this.#map,selection:this.#selection});
    this.#show(step.map,step.selection,'redo');
    return true;
  }

  // ---- drags: many previews, one undo step ------------------------------------------------------------------------
  begin(){this.cancel();this.#drag={map:this.#map,selection:this.#selection,error:null}}
  /** Shows change(map as it was when the drag began) without recording it. An invalid result keeps the last good frame. */
  preview(change){
    if(!this.#drag)this.begin();
    const drag=this.#drag;
    let next;
    try{next=this.#canonical(change(drag.map))}catch(error){const result=refuse(error);drag.error=result.error;return result}
    drag.error=null;
    this.#show(next,this.#selection,'preview');
    return {ok:true};
  }
  /** Ends the drag: records one undo step, or restores the start and says why when the last frame was invalid. */
  commit(){
    const drag=this.#drag;
    if(!drag)return {ok:true};
    this.#drag=null;
    if(drag.error!==null){this.#show(drag.map,drag.selection,'cancel');return {ok:false,error:drag.error}}
    if(sameMap(this.#map,drag.map)){this.#show(drag.map,this.#selection,'cancel');return {ok:true}}
    // The last preview is already showing; it only needs recording.
    this.#undo.push({map:drag.map,selection:drag.selection});
    if(this.#undo.length>HISTORY)this.#undo.shift();
    this.#redo=[];
    this.#emit('map','edit');
    return {ok:true};
  }
  cancel(){
    const drag=this.#drag;
    if(!drag)return;
    this.#drag=null;
    this.#show(drag.map,drag.selection,'cancel');
  }

  // ---- pure edits: (map, …) → map, for apply() and preview() ------------------------------------------------------
  #rebuild(map,ids,change){
    const set=ids instanceof Set?ids:new Set(ids),out={...map};
    let left=set.size;
    for(const kind of KINDS){
      if(!left)break;
      const items=map[kind];let list=null;
      for(let i=0;i<items.length;i++){
        const item=items[i];
        if(!set.has(item.id))continue;
        left--;
        const next=change(item,kind,i);
        if(next!==item)(list??=items.slice())[i]=next;
      }
      if(list){this.#known.add(list);out[kind]=list}
    }
    return out;
  }
  /** The map with the listed objects shifted by (dx, dz). */
  moved(map,ids,dx,dz){
    dx=trim(dx);dz=trim(dz);
    if(!dx&&!dz)return map;
    // The objects are already valid, so only their new coordinates need checking.
    const place=(v,kind,i)=>{v=trim(v);if(v<-LIMITS.extent||v>LIMITS.extent)throw new MapError(`${label(kind,i)} would leave the ${LIMITS.extent} m limit`);return v};
    return this.#rebuild(map,ids,(item,kind,i)=>{
      const made=item.rect
        ?{...item,rect:[place(item.rect[0]+dx,kind,i),place(item.rect[1]+dz,kind,i),place(item.rect[2]+dx,kind,i),place(item.rect[3]+dz,kind,i)]}
        :{...item,p:[place(item.p[0]+dx,kind,i),place(item.p[1]+dz,kind,i)]};
      this.#known.add(made);
      return made;
    });
  }
  /** The map with fields changed on one object, or on several: then each takes only the fields it already has. */
  patched(map,ids,patch){
    const many=Array.isArray(ids);
    return this.#rebuild(map,many?ids:[ids],(item,kind,i)=>{
      const fields=many?Object.fromEntries(Object.entries(patch).filter(([key])=>key in item)):patch;
      return Object.keys(fields).length?this.#mint(kind,{...item,...fields},i,item.id):item;
    });
  }
  #append(map,entries,dx,dz){
    const out={...map},made=[];
    for(const {kind,item} of entries){
      if(!KINDS.includes(kind))throw new MapError(`Unknown kind of object: ${kind}`);
      if(out[kind]===map[kind])out[kind]=map[kind].slice();
      const copy=this.#mint(kind,offset(bare(item),dx,dz),out[kind].length);
      out[kind].push(copy);made.push(copy.id);
    }
    return [out,made];
  }
  #grow(entries,dx,dz){
    let ids=[];
    const result=this.apply(map=>{let out;[out,ids]=this.#append(map,entries,dx,dz);return out},{limits:true,select:()=>ids});
    return result.ok?{ok:true,ids}:result;
  }

  // ---- editing operations ---------------------------------------------------------------------------------------
  move(ids,dx,dz){return this.apply(map=>this.moved(map,ids,dx,dz))}
  patch(ids,patch){return this.apply(map=>this.patched(map,ids,patch))}
  remove(ids){
    const set=new Set(ids);
    return this.apply(map=>{
      const out={...map};
      for(const kind of KINDS)if(map[kind].some(item=>set.has(item.id)))out[kind]=map[kind].filter(item=>!set.has(item.id));
      return out;
    });
  }
  /** Adds an object, filling in defaults, and selects it. Returns {ok, id}. */
  add(kind,item){
    if(!KINDS.includes(kind)||!item||typeof item!=='object')return {ok:false,error:`Unknown kind of object: ${kind}`};
    const result=this.#grow([{kind,item}],0,0);
    return result.ok?{ok:true,id:result.ids[0]}:result;
  }
  /** Copies the listed objects, offset by (dx, dz), and selects the copies. Returns {ok, ids} in the order given. */
  duplicate(ids,dx=0,dz=0){return this.#grow(this.copy(ids),dx,dz)}
  /** A clipboard entry list for the listed objects: plain data that survives JSON. */
  copy(ids){return ids.map(id=>this.find(id)).filter(Boolean).map(({kind,item})=>({kind,item:bare(item)}))}
  /** Adds clipboard entries, centred on a point when one is given, and selects them. Returns {ok, ids}. */
  paste(clip,at){
    if(!Array.isArray(clip)||!clip.length||!clip.every(entry=>entry&&KINDS.includes(entry.kind)&&entry.item&&(Array.isArray(entry.item.rect)||Array.isArray(entry.item.p))))return {ok:false,error:'Nothing to paste'};
    const box=boxOf(clip.map(entry=>entry.item));
    return this.#grow(clip,at?at[0]-(box[0]+box[2])/2:0,at?at[1]-(box[1]+box[3])/2:0);
  }
  /**
   * Changes any of name, theme, mode, build, respawn, bounds and nav ({cell, clearance}). A map switched to Forts is made playable
   * (`fortified`); one switched back keeps its zones and its starts' teams, which a siege ignores. `respawn` is
   * [seconds a gun pickup takes to come back, seconds a box of ammunition takes]; null takes it out, so the game's defaults apply.
   */
  setSettings(patch){
    return this.apply(map=>{
      const next={...map,name:patch.name??map.name,theme:patch.theme??map.theme,mode:patch.mode??map.mode,build:patch.build??map.build,
        respawn:patch.respawn===undefined?map.respawn:patch.respawn,cabinets:patch.cabinets??map.cabinets,bounds:patch.bounds??map.bounds,nav:patch.nav?{...map.nav,...patch.nav}:map.nav};
      return next.mode==='forts'&&map.mode!=='forts'?fortified(next):next;
    });
  }

  // ---- selection --------------------------------------------------------------------------------------------------
  select(ids,{add=false,toggle=false}={}){
    let next;
    if(toggle){const flip=new Set(ids);next=[...this.#selection.filter(id=>!flip.has(id)),...ids.filter(id=>!this.#selection.includes(id))]}
    else next=add?[...this.#selection,...ids]:[...ids];
    this.#show(this.#map,[...new Set(next)],'select');
  }
  selectAll(kinds=KINDS){this.select(kinds.flatMap(kind=>this.#map[kind].map(item=>item.id)))}

  // ---- picking and snapping -----------------------------------------------------------------------------------------
  /**
   * The topmost object within tolerance of a point, or null; of two on one layer the smaller wins, so a marker or a chair is picked over
   * a napkin it stands on. options: marker (plan radius of point objects), kinds.
   */
  pick(x,z,tolerance=0,{marker=MARKER,kinds=KINDS}={}){
    let best=null,bestLayer=-Infinity,bestArea=Infinity;
    for(const kind of kinds)for(const item of this.#map[kind]){
      const r=item.rect,[x0,z0,x1,z1]=itemBox(item,marker);
      if(x<x0-tolerance||x>x1+tolerance||z<z0-tolerance||z>z1+tolerance)continue;
      const layer=r?LAYER[kind]:4,area=(x1-x0)*(z1-z0);
      if(layer>bestLayer||(layer===bestLayer&&area<=bestArea)){best=item.id;bestLayer=layer;bestArea=area}
    }
    return best;
  }
  /** Ids of every object a rectangle touches; a Forts area (a base, the forcefield) only when the rectangle holds all of it. */
  pickAll(rect,{marker=MARKER,kinds=KINDS}={}){
    const ids=[];
    for(const kind of kinds)for(const item of this.#map[kind]){
      const b=itemBox(item,marker);
      if(kind==='zones'&&item.rect?b[0]>=rect[0]&&b[2]<=rect[2]&&b[1]>=rect[1]&&b[3]<=rect[3]:b[0]<=rect[2]&&b[2]>=rect[0]&&b[1]<=rect[3]&&b[3]>=rect[1])ids.push(item.id);
    }
    return ids;
  }
  /** Sorted x and z coordinates a dragged edge can snap to: the arena and every rectangle not being dragged. */
  snapEdges(exclude=[]){
    const skip=new Set(exclude),b=this.#map.bounds,x=new Set([b[0],b[2]]),z=new Set([b[1],b[3]]);
    for(const kind of KINDS)for(const item of this.#map[kind])if(item.rect&&!skip.has(item.id)){x.add(item.rect[0]);x.add(item.rect[2]);z.add(item.rect[1]);z.add(item.rect[3])}
    const sorted=set=>[...set].sort((a,b)=>a-b);
    return {x:sorted(x),z:sorted(z)};
  }
}
