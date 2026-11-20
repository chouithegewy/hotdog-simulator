// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The sandbox link composer's sums (dev.html): the address of a game, from the choices on the page.
//   compose({host:'192.0.2.10',port:8141,weapons:['mustard','sniper'],ammo:true,bots:2,botmode:'idle'})
//   → http://192.0.2.10:8141/?mp&sandbox&weapons=mustard,sniper&ammo=inf&bots=2&botmode=idle
// ?mp (online) or ?play (offline) comes first, then sandbox, then the rest in a fixed order; a switch is a bare key
// (&god), anything else key=value. The sandbox's own settings (guns, ammo, bots…) are left out of a game that is not a sandbox.

/** The guns, as the game numbers them, with the names the address uses and the game shows. */
export const WEAPONS=[{name:'mustard',id:1,title:'Mustard pistol'},{name:'sniper',id:2,title:'Ketchup sniper'},{name:'hotsauce',id:3,title:'Hot sauce thrower'},{name:'relish',id:4,title:'Relish bazooka'},{name:'bugspray',id:5,title:'Bug spray'},{name:'syrup',id:6,title:'Syrup rifle'},{name:'mayo',id:7,title:'Mayo squirt bottle'}];
/** The modes, as the address names them and the page shows them. */
export const MODES=[['siege','Siege'],['forts','Forts'],['ffa','Free For All'],['cards','Fully Loaded']];
/** The one map Fully Loaded is played on (the lobby's `CARDS_MAP`): its map list offers that and nothing else. */
export const CARDS_MAP='counter-siege';
/** The game servers there usually are: the live one and the two spikes. */
export const PORTS=[[8137,'live'],[8141,'spike A'],[8142,'spike B']];
export const BOTMODES=['idle','walk','fight'],RESPAWNS=['start','here'];
/** Everything a link can say, as it is when nothing is chosen. */
export const BLANK={protocol:'http:',host:'localhost',port:8137,online:true,sandbox:true,mode:'',map:'',weapons:[],ammo:false,god:false,bots:null,botmode:'fight',zombies:false,field:false,stim:false,respawn:'start',check:false,webgl2:false};
/** The quick links: a few games worth having one tap away, on every server. */
export const QUICK=[
 {label:'Mustard + sniper',title:'Sandbox: the mustard pistol and the ketchup sniper',options:{weapons:['mustard','sniper']}},
 {label:'Every gun · god · idle bots',title:'Sandbox: every gun, unlimited ammo, god mode, four bots standing still',options:{weapons:'all',ammo:true,god:true,bots:4,botmode:'idle'}},
 {label:'Sandbox FFA',title:'Sandbox: Free For All',options:{mode:'ffa'}},
 {label:'Plain FFA',title:'Free For All, not a sandbox: the lobby\'s leader chooses it',options:{sandbox:false,mode:'ffa'}}];

// A host as an address takes it: no scheme, port or path pasted along with it (the port is chosen apart), an IPv6 address in brackets.
const hostOf=text=>{const h=String(text??'').trim().replace(/^[a-z][\w+.-]*:\/\//i,'').replace(/[/?#].*$/,'').replace(/^(\[[^\]]*\]):\d*$/,'$1').replace(/^([^:]*):\d*$/,'$1')||'localhost';
 return h.split(':').length>2&&!h.startsWith('[')?`[${h}]`:h};
const value=v=>encodeURIComponent(String(v)).replace(/%2C/gi,',');
/** The guns a link names: 'all', or the known names in the game's order; '' for none (the game's own). */
export const weaponList=weapons=>{if(weapons==='all'||(Array.isArray(weapons)&&weapons.includes('all')))return 'all';const asked=new Set(Array.isArray(weapons)?weapons:String(weapons??'').split(','));return WEAPONS.filter(w=>asked.has(w.name)).map(w=>w.name).join(',')};
/** The query of a link (from ?mp or ?play on), from the choices (see BLANK); unknown or default choices are left out. */
export function query(options){const o={...BLANK,...options},parts=[o.online?'mp':'play'];
 if(o.sandbox)parts.push('sandbox');
 if(MODES.some(([m])=>m===o.mode))parts.push(`mode=${o.mode}`);
 const map=String(o.map??'').replace(/\.json$/,'');if(/^[\w-]+$/.test(map))parts.push(`map=${value(map)}`);
 if(o.sandbox){const guns=weaponList(o.weapons);if(guns)parts.push(`weapons=${guns}`);
  if(o.ammo)parts.push('ammo=inf');if(o.god)parts.push('god');
  const bots=o.bots===''||o.bots==null?NaN:Number(o.bots);if(Number.isInteger(bots)&&bots>=0&&bots<=4)parts.push(`bots=${bots}`);
  if(BOTMODES.includes(o.botmode)&&o.botmode!=='fight')parts.push(`botmode=${o.botmode}`);
  if(o.zombies)parts.push('zombies=on');if(o.field)parts.push('field=on');if(o.stim)parts.push('stim=inf');
  if(o.respawn==='here')parts.push('respawn=here')}
 if(o.check)parts.push('check');if(o.webgl2)parts.push('webgl2');
 return '?'+parts.join('&')}
/** The whole link: http://HOST:PORT/?mp&sandbox&… */
export function compose(options){const o={...BLANK,...options},port=Number(o.port),protocol=o.protocol==='https:'?'https:':'http:';
 return `${protocol}//${hostOf(o.host)}${Number.isInteger(port)&&port>0&&port<65536?`:${port}`:''}/${query(o)}`}
