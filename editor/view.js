// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// Plan view of a map on a canvas: x runs right, z runs down, one world unit is a metre.
import {KITS,FIRST_KITS,WEAPONS,AMMO_BOX,SINK_DEPTH,SINK_MAX,ROUND} from '../tools/network/map.mjs';
import {MARKER,FOOTPRINT,FIXED,KIT_NAME,itemBox,reach} from './model.js';

// The materials, then the appliances and the things that stand on the counter (each in the colour it mostly is: a red enamel kettle, a yellow box of cereal…).
export const STYLE={steel:'#aab6ba',stone:'#8e918a',wood:'#c2935a',dark:'#3d4447',white:'#f3f2ec',red:'#c8402c',green:'#5d9447',yellow:'#e9b824',metal:'#8ea0a8',microwave:'#5b676e',toaster:'#cdd2d3',
  kettle:'#c4412e','knife-block':'#bd8a54',canister:'#e6e0d2',jar:'#cfe3e8',cereal:'#e8a317','paper-towel':'#f6f4ee','dish-rack':'#d9dcda','spice-rack':'#6b4428','fruit-bowl':'#3a8f8a',faucet:'#c3cbd0',griddle:'#4b5256',
  bamboo:'#d8bf7e',gong:'#2b2e30'};
// A sink: its steel flange and basin, the basin's dark depths and the drain.
const SINK={rim:'#b9c2c6',edge:'#6f7c82',light:'#75828a',deep:'#323c40',drain:'#a3adb2',hole:'#11171a'};
// A pedal bin: brushed steel, black base, lid hinge and pedal.
const BIN={light:'#e2e6e8',dark:'#8a959a',black:'#15191b',ring:'#5d686d'};
// Little things the counter things carry: lids, fruit, spice jars.
const BITS={steel:'#a9b0b5',black:'#1d2124',walnut:'#5a3a22',beech:'#bd8a54',card:'#94704a',white:'#fbfaf6',glass:'#7fa3ab',
  fruit:['#ff8514','#b80d0d','#7ab829'],banana:'#f2d04a',spice:['#c71a12','#388c33','#6b3d1a','#f7bd1f','#f57514','#eddba8'],mug:['#c71a12','#ede8dc']};
// The five hotdogs' identity colours, as in kitchen_sim::feast::COLORS.
export const PLAYER=['#38c9e3','#ff853b','#fad138','#e8456b','#a17af5'];
export const LOOSE_COLOR={bowl:'#c9d0d2',crate:'#c2935a',pillow:'#e8e1d0',napkin:'#d9484c',chair:'#a8743f',tray:'#b9c1c4',cereal:'#f08e22',cup:'#1f6f78',plate:'#f4f4f0',tin:'#8aa59a',board:'#d0a46c',
  dish:'#7f9a86',roll:'#f7f6f1',salt:'#e9eef0',pepper:'#3b3a38',breadbox:'#efe5cf'};
// What a pickup holds, by weapon number: the ammo box (0) in olive drab, then the mustard pistol, ketchup sniper, hot sauce thrower,
// relish bazooka, bug spray, syrup rifle and mayo squirter. A number the editor does not know is drawn in grey.
export const WEAPON_COLOR=['#78803a','#f0b323','#c4321c','#e2582a','#4f8f3a','#5aa9c9','#a65f1c','#efe6c4'],UNKNOWN_WEAPON='#9aa5a8';
export const weaponColor=weapon=>WEAPON_COLOR[weapon]??UNKNOWN_WEAPON;
// The brass case and copper tip of the cartridges drawn on an ammo box.
const BRASS='#e2b04a',COPPER='#b8693a';
// A Forts map: the two teams' colours and short names, the forcefield's blue, and the bacon's lean and fat.
export const TEAM_COLOR={mustard:'#e0b020',ketchup:'#c8281e'},TEAM_LABEL={mustard:'Mustard',ketchup:'Ketchup'},FIELD='#2f8fe6',BACON=['#b23a2c','#f1d9b5'];
/** What a zone is called: "Mustard base", "Forcefield", "Bacon". */
export const zoneName=zone=>zone.kind==='base'?`${TEAM_LABEL[zone.team]??'Team'} base`:zone.kind==='field'?'Forcefield':'Bacon';
const INK='#18242a',SELECT='#1f5fbf',KETCHUP='#c4321c',MUSTARD='#f0b323',COUNTER='#e9e4d4',UNDER='#283233',RULER=20;
const FONT='"Barlow Semi Condensed","Arial Narrow",sans-serif';
const shade=(hex,amount)=>{
  const n=parseInt(hex.slice(1),16),mix=c=>Math.round(amount<0?c*(1+amount):c+(255-c)*amount);
  return `rgb(${mix(n>>16)},${mix(n>>8&255)},${mix(n&255)})`;
};
const dark=hex=>{const n=parseInt(hex.slice(1),16);return (n>>16)*.3+(n>>8&255)*.59+(n&255)*.11<120};
/** Direction a yaw faces on the plan: the game's forward is -z, turned about the vertical axis. */
export const facing=yaw=>[-Math.sin(yaw),-Math.cos(yaw)];
export const yawTowards=(dx,dz)=>Math.atan2(-dx,-dz);
const SIDE={'+x':[1,0],'-x':[-1,0],'+z':[0,1],'-z':[0,-1]};
// The turn on the plan that brings a counter thing's front from up the screen (-z) round to the side it faces.
const TURN={'-z':0,'+z':Math.PI,'+x':Math.PI/2,'-x':-Math.PI/2};
const OVERLAYS={
  reach:(a,id)=>a.stand[id]?(a.reach[id]?[63,125,52,82]:[196,50,28,96]):null,
  zombie:(a,id,far)=>a.zdist[id]<0?(a.blockTop[id]?null:[24,36,42,70]):[70+150*(a.zdist[id]/far),132-70*(a.zdist[id]/far),40,150-95*(a.zdist[id]/far)],
  // Raised floor in blue; a sink's floor, below the counter, in rust, darker the deeper it is.
  height:(a,id,far,top)=>a.groundH[id]>0?[31,95,191,40+150*a.groundH[id]/top]:a.groundH[id]<0?[176,84,30,70+140*Math.min(1,-a.groundH[id]/SINK_MAX)]:null,
  nav:(a,id,far,top,level)=>level.blocked[id]?[24,36,42,105]:null,
};

export class View {
  constructor(canvas){
    this.canvas=canvas;this.ctx=canvas.getContext('2d');
    this.scale=240;this.cx=0;this.cz=0;this.w=1;this.h=1;this.dpr=1;this.overlay=null;
  }
  resize(){
    const box=this.canvas.getBoundingClientRect();
    this.dpr=window.devicePixelRatio||1;this.w=Math.max(1,box.width);this.h=Math.max(1,box.height);
    this.canvas.width=Math.round(this.w*this.dpr);this.canvas.height=Math.round(this.h*this.dpr);
  }
  sx(x){return (x-this.cx)*this.scale+this.w/2}
  sy(z){return (z-this.cz)*this.scale+this.h/2}
  toWorld(px,py){return [(px-this.w/2)/this.scale+this.cx,(py-this.h/2)/this.scale+this.cz]}
  fit(box,pad=48){
    const width=Math.max(.05,box[2]-box[0]),depth=Math.max(.05,box[3]-box[1]);
    this.scale=Math.min(8000,Math.max(4,Math.min((this.w-RULER-2*pad)/width,(this.h-RULER-2*pad)/depth)));
    this.cx=(box[0]+box[2])/2-RULER/2/this.scale;this.cz=(box[1]+box[3])/2-RULER/2/this.scale;
  }
  zoomAt(px,py,factor){
    const [x,z]=this.toWorld(px,py);
    this.scale=Math.min(8000,Math.max(4,this.scale*factor));
    this.cx=x-(px-this.w/2)/this.scale;this.cz=z-(py-this.h/2)/this.scale;
  }
  /** Plan radius of point markers: their real size, but never smaller than 9 px on screen. */
  marker(){return Math.max(MARKER,9/this.scale)}
  handles(rect){
    const xs=[rect[0],(rect[0]+rect[2])/2,rect[2]],zs=[rect[1],(rect[1]+rect[3])/2,rect[3]],out=[];
    for(let j=0;j<3;j++)for(let i=0;i<3;i++)if(i!==1||j!==1)out.push({x:i-1,z:j-1,px:this.sx(xs[i]),py:this.sy(zs[j])});
    // On a small rectangle the middle handles would cover the corners.
    const wide=(rect[2]-rect[0])*this.scale>=30,deep=(rect[3]-rect[1])*this.scale>=30;
    return out.filter(h=>(h.x!==0||wide)&&(h.z!==0||deep));
  }
  handleAt(rect,px,py){
    let best=null,distance=8;
    for(const handle of this.handles(rect)){const d=Math.max(Math.abs(handle.px-px),Math.abs(handle.py-py));if(d<=distance){best=handle;distance=d}}
    return best;
  }

  #overlayImage(doc,mode){
    const analysis=doc.analysis,level=doc.level;
    if(this.overlay&&this.overlay.analysis===analysis&&this.overlay.mode===mode)return this.overlay.image;
    const {cols,rows}=level,image=document.createElement('canvas');
    image.width=cols;image.height=rows;
    const ctx=image.getContext('2d'),data=ctx.createImageData(cols,rows),pixels=new Uint32Array(data.data.buffer),paint=OVERLAYS[mode];
    let far=.001,top=.001;
    for(let id=0;id<cols*rows;id++){if(analysis.zdist[id]>far)far=analysis.zdist[id];if(analysis.groundH[id]>top)top=analysis.groundH[id]}
    for(let id=0;id<cols*rows;id++){
      const c=paint(analysis,id,far,top,level);
      if(c)pixels[id]=(c[3]<<24|c[2]<<16|c[1]<<8|c[0])>>>0;
    }
    ctx.putImageData(data,0,0);
    this.overlay={analysis,mode,image};
    return image;
  }

  /** scene: {doc, overlay, step, hover, selection, guides, marquee, ghost, readout, badges, liveOverlay} */
  draw(scene){
    const {ctx,w,h}=this,{doc}=scene,map=doc.map,b=map.bounds,marker=this.marker(),r=marker*this.scale;
    ctx.setTransform(this.dpr,0,0,this.dpr,0,0);
    ctx.fillStyle=UNDER;ctx.fillRect(0,0,w,h);
    const ax=this.sx(b[0]),az=this.sy(b[1]),aw=(b[2]-b[0])*this.scale,ah=(b[3]-b[1])*this.scale;
    ctx.fillStyle='#0005';ctx.fillRect(ax+3,az+5,aw,ah);
    ctx.fillStyle=COUNTER;ctx.fillRect(ax,az,aw,ah);
    const selected=new Set(scene.selection),visible=rect=>this.sx(rect[2])>=-40&&this.sx(rect[0])<=w+40&&this.sy(rect[3])>=-40&&this.sy(rect[1])<=h+40;
    // A sink is a hole in the counter, under everything, and under the overlays, which say what its floor is like.
    for(const sink of map.sinks??[])if(visible(sink.rect))this.#sink(sink);

    ctx.save();ctx.beginPath();ctx.rect(ax,az,aw,ah);ctx.clip();
    if(scene.overlay!=='none'&&(scene.liveOverlay||this.overlay)){
      const level=doc.level,image=scene.liveOverlay?this.#overlayImage(doc,scene.overlay):this.overlay.image;
      ctx.imageSmoothingEnabled=false;
      ctx.drawImage(image,ax,az,image.width*level.cell*this.scale,image.height*level.cell*this.scale);
    }
    this.#grid(scene.step,b);
    ctx.restore();
    ctx.strokeStyle='#0b1418';ctx.lineWidth=1.5;ctx.strokeRect(ax,az,aw,ah);

    const point=p=>[p[0]-marker,p[1]-marker,p[0]+marker,p[1]+marker];
    // A Forts map's bases lie on the counter under everything; its forcefield and bacon are drawn over the level.
    for(const zone of map.zones)if(zone.kind==='base'&&visible(zone.rect))this.#zone(zone);
    for(const deck of map.decks)if(visible(deck.rect))this.#deck(deck);
    for(const ramp of map.ramps)if(visible(ramp.rect))this.#ramp(ramp);
    for(const block of map.blocks)if(visible(block.rect))this.#block(block);
    for(const prop of map.props)if(prop.rect){if(visible(prop.rect))this.#box(prop)}
    for(const zone of map.zones)if(zone.kind==='field'&&visible(zone.rect))this.#zone(zone);
    for(const zone of map.zones)if(zone.kind==='bacon'&&visible(point(zone.p)))this.#bacon(zone,r);
    for(const prop of map.props)if(!prop.rect&&visible(itemBox(prop,marker))){if(prop.kind==='turret')this.#turret(prop,r);else if(prop.kind==='bin')this.#bin(prop);else this.#loose(prop)}
    map.pickups.forEach(pickup=>{if(visible(point(pickup.p)))this.#pickup(pickup,r)});
    map.spawns.forEach(spawn=>{if(visible(point(spawn.p)))this.#spawn(spawn,r)});
    map.starts.forEach((start,index)=>{if(visible(point(start.p)))this.#start(start,index,r)});
    if(scene.ghost)this.#ghost(scene.ghost,r);

    // Selection, hover and problem marks sit above every object.
    const outline=(item,colour,width)=>{
      ctx.strokeStyle=colour;ctx.lineWidth=width;
      if(item.rect)ctx.strokeRect(this.sx(item.rect[0]),this.sy(item.rect[1]),(item.rect[2]-item.rect[0])*this.scale,(item.rect[3]-item.rect[1])*this.scale);
      else if(FOOTPRINT[item.kind]&&item.kind!=='bowl'){
        // A loose object other than the round bowl is outlined round its footprint, turned as it is.
        const [hx,hz]=FOOTPRINT[item.kind],s=this.scale;
        ctx.save();ctx.translate(this.sx(item.p[0]),this.sy(item.p[1]));ctx.rotate(-item.yaw);ctx.strokeRect(-hx*s-4,-hz*s-4,2*hx*s+8,2*hz*s+8);ctx.restore();
      }
      else if(FIXED[item.kind]){
        // A bin is an oval.
        const [hx,hz]=FIXED[item.kind],s=this.scale;
        ctx.save();ctx.translate(this.sx(item.p[0]),this.sy(item.p[1]));ctx.rotate(-item.yaw);ctx.beginPath();ctx.ellipse(0,0,hx*s+4,hz*s+4,0,0,7);ctx.stroke();ctx.restore();
      }
      else{ctx.beginPath();ctx.arc(this.sx(item.p[0]),this.sy(item.p[1]),reach(item,marker)*this.scale+4,0,7);ctx.stroke()}
    };
    if(scene.hover!==null&&!selected.has(scene.hover)){const found=doc.find(scene.hover);if(found)outline(found.item,'#1f5fbf99',1.5)}
    let single=null;
    for(const id of scene.selection){
      const found=doc.find(id);if(!found)continue;
      const box=itemBox(found.item,marker);
      if(!visible(box))continue;
      ctx.setLineDash([]);outline(found.item,'#fff',4);outline(found.item,SELECT,2);
      single=found.item;
    }
    if(scene.selection.length===1&&single?.rect)for(const handle of this.handles(single.rect)){
      ctx.fillStyle='#fff';ctx.strokeStyle=SELECT;ctx.lineWidth=1.5;
      ctx.fillRect(handle.px-4,handle.py-4,8,8);ctx.strokeRect(handle.px-4,handle.py-4,8,8);
    }
    for(const [id,severity] of scene.badges){
      const found=doc.find(id);if(!found)continue;
      const box=itemBox(found.item,marker),x=this.sx(box[2]),y=this.sy(box[1]);
      if(x<0||x>w||y<0||y>h)continue;
      ctx.beginPath();ctx.arc(x,y,7,0,7);ctx.fillStyle=severity==='error'?KETCHUP:MUSTARD;ctx.fill();ctx.strokeStyle='#fff';ctx.lineWidth=1.5;ctx.stroke();
      ctx.fillStyle=severity==='error'?'#fff':INK;ctx.font=`600 11px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='middle';ctx.fillText('!',x,y+.5);
    }

    if(scene.marquee){
      const m=scene.marquee,x=this.sx(m[0]),y=this.sy(m[1]),mw=(m[2]-m[0])*this.scale,mh=(m[3]-m[1])*this.scale;
      ctx.fillStyle='#1f5fbf1f';ctx.fillRect(x,y,mw,mh);ctx.strokeStyle=SELECT;ctx.lineWidth=1;ctx.setLineDash([5,4]);ctx.strokeRect(x+.5,y+.5,mw,mh);ctx.setLineDash([]);
    }
    ctx.strokeStyle='#e0356b';ctx.lineWidth=1;
    if(scene.guides.x!==null){const x=Math.round(this.sx(scene.guides.x))+.5;ctx.beginPath();ctx.moveTo(x,RULER);ctx.lineTo(x,h);ctx.stroke()}
    if(scene.guides.z!==null){const y=Math.round(this.sy(scene.guides.z))+.5;ctx.beginPath();ctx.moveTo(RULER,y);ctx.lineTo(w,y);ctx.stroke()}
    if(scene.readout){
      const {text,px,py}=scene.readout;
      ctx.font=`600 13px ${FONT}`;ctx.textAlign='left';ctx.textBaseline='middle';
      const tw=ctx.measureText(text).width,x=Math.min(w-tw-18,px+14),y=Math.max(RULER+14,py-18);
      ctx.fillStyle=INK;ctx.beginPath();ctx.roundRect(x-6,y-10,tw+12,20,5);ctx.fill();
      ctx.fillStyle='#fff';ctx.fillText(text,x,y+.5);
    }
    this.#rulers();
  }

  #grid(step,b){
    const {ctx}=this,lines=(size,colour)=>{
      if(size*this.scale<7)return;
      ctx.strokeStyle=colour;ctx.lineWidth=1;ctx.beginPath();
      const [x0,z0]=this.toWorld(0,0),[x1,z1]=this.toWorld(this.w,this.h);
      for(let i=Math.ceil(Math.max(x0,b[0])/size);i*size<=Math.min(x1,b[2]);i++){const x=Math.round(this.sx(i*size))+.5;ctx.moveTo(x,0);ctx.lineTo(x,this.h)}
      for(let j=Math.ceil(Math.max(z0,b[1])/size);j*size<=Math.min(z1,b[3]);j++){const y=Math.round(this.sy(j*size))+.5;ctx.moveTo(0,y);ctx.lineTo(this.w,y)}
      ctx.stroke();
    };
    if(step>0&&step<.1)lines(step,'#18242a0d');
    lines(.1,'#18242a1a');
    lines(.5,'#18242a33');
  }
  #rulers(){
    const {ctx,w,h}=this,steps=[.01,.02,.05,.1,.2,.5,1,2,5,10,20,50],major=steps.find(s=>s*this.scale>=64)??50,minor=major/5;
    ctx.fillStyle='#dfe4e3';ctx.fillRect(0,0,w,RULER);ctx.fillRect(0,0,RULER,h);
    ctx.strokeStyle='#8d9a9b';ctx.fillStyle='#35454b';ctx.lineWidth=1;ctx.font=`500 11.5px ${FONT}`;ctx.textBaseline='middle';
    const name=v=>String(+v.toFixed(2)),[x0,z0]=this.toWorld(RULER,RULER),[x1,z1]=this.toWorld(w,h);
    ctx.beginPath();ctx.textAlign='left';
    for(let i=Math.ceil(x0/minor);i*minor<=x1;i++){
      const x=Math.round(this.sx(i*minor))+.5,whole=i%5===0;
      ctx.moveTo(x,RULER);ctx.lineTo(x,whole?4:RULER-5);
      if(whole)ctx.fillText(name(i*minor),x+3,RULER/2-1);
    }
    for(let j=Math.ceil(z0/minor);j*minor<=z1;j++){
      const y=Math.round(this.sy(j*minor))+.5,whole=j%5===0;
      ctx.moveTo(RULER,y);ctx.lineTo(whole?4:RULER-5,y);
      if(whole){ctx.save();ctx.translate(RULER/2-1,y-3);ctx.rotate(-Math.PI/2);ctx.fillText(name(j*minor),0,0);ctx.restore()}
    }
    ctx.stroke();
    ctx.fillStyle='#dfe4e3';ctx.fillRect(0,0,RULER,RULER);
    ctx.fillStyle='#35454b';ctx.textAlign='center';ctx.fillText('m',RULER/2,RULER/2);
    ctx.strokeStyle='#a9b5b4';ctx.beginPath();ctx.moveTo(0,RULER+.5);ctx.lineTo(w,RULER+.5);ctx.moveTo(RULER+.5,0);ctx.lineTo(RULER+.5,h);ctx.stroke();
  }
  #screen(rect){return [this.sx(rect[0]),this.sy(rect[1]),(rect[2]-rect[0])*this.scale,(rect[3]-rect[1])*this.scale]}
  #label(text,x,y,width,colour){
    const {ctx}=this;
    ctx.font=`600 12.5px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='middle';
    if(ctx.measureText(text).width+8<=width){ctx.fillStyle=colour;ctx.fillText(text,x,y)}
  }
  #deck(deck){
    if(deck.style===ROUND){this.#steamer(deck);return}
    const {ctx}=this,[x,y,w,h]=this.#screen(deck.rect),colour=STYLE[deck.style];
    ctx.fillStyle=colour;ctx.fillRect(x,y,w,h);
    // Tread-plate hatching marks a floor you stand on, as opposed to a block you walk around.
    ctx.save();ctx.beginPath();ctx.rect(x,y,w,h);ctx.clip();
    ctx.strokeStyle=dark(colour)?'#ffffff26':'#ffffff59';ctx.lineWidth=2;ctx.beginPath();
    for(let d=-h;d<w;d+=9){ctx.moveTo(x+d,y+h);ctx.lineTo(x+d+h,y)}
    ctx.stroke();ctx.restore();
    ctx.strokeStyle=shade(colour,-.45);ctx.lineWidth=1.5;ctx.strokeRect(x,y,w,h);
    if(h>=18)this.#label(`Deck ${deck.height} m`,x+w/2,y+h-9,w,dark(colour)?'#fff':INK);
  }
  /** A slim bamboo block: a chopstick standing in its holder, seen from above, its block's box dashed round it. */
  #chopstick(block){
    const {ctx}=this,[x,y,w,h]=this.#screen(block.rect),cx=x+w/2,cy=y+h/2,r=Math.min(w,h)/2;
    ctx.fillStyle='#0f191d2e';ctx.beginPath();ctx.arc(cx+1.5,cy+2,r,0,7);ctx.fill();
    ctx.beginPath();ctx.arc(cx,cy,r,0,7);ctx.fillStyle=BITS.white;ctx.fill();ctx.strokeStyle='#2f5f9e';ctx.lineWidth=Math.max(1,r*.18);ctx.stroke();
    ctx.beginPath();ctx.arc(cx,cy,Math.max(1.5,r*.32),0,7);ctx.fillStyle=STYLE.bamboo;ctx.fill();ctx.strokeStyle=shade(STYLE.bamboo,-.5);ctx.lineWidth=1;ctx.stroke();
    ctx.strokeStyle=shade(STYLE.bamboo,-.5);ctx.lineWidth=1;ctx.setLineDash([3,2]);ctx.strokeRect(x,y,w,h);ctx.setLineDash([]);
  }
  /** A round deck (bamboo): a steamer's lid, the ellipse inside its rect, woven in rings, a red rim round its edge; the rect dashed round it. */
  #steamer(deck){
    const {ctx}=this,[x,y,w,h]=this.#screen(deck.rect),colour=STYLE[deck.style],cx=x+w/2,cy=y+h/2;
    const oval=(k,fill,stroke,width=1)=>{ctx.beginPath();ctx.ellipse(cx,cy,w/2*k,h/2*k,0,0,7);if(fill){ctx.fillStyle=fill;ctx.fill()}if(stroke){ctx.strokeStyle=stroke;ctx.lineWidth=width;ctx.stroke()}};
    ctx.fillStyle='#0f191d2e';ctx.beginPath();ctx.ellipse(cx+2,cy+3,w/2,h/2,0,0,7);ctx.fill();
    oval(1,colour,shade(colour,-.5),1.5);
    for(let k=.88;k>.1;k-=.12)oval(k,null,shade(colour,-.18),1);
    oval(.97,null,STYLE.red,Math.max(1.5,Math.min(w,h)*.015));
    ctx.strokeStyle=shade(colour,-.5);ctx.lineWidth=1;ctx.setLineDash([3,2]);ctx.strokeRect(x,y,w,h);ctx.setLineDash([]);
    if(h>=18)this.#label(`Deck ${deck.height} m`,cx,cy,w*.7,INK);
  }
  #ramp(ramp){
    const {ctx}=this,[x,y,w,h]=this.#screen(ramp.rect),[dx,dz]=SIDE[ramp.rise],cx=x+w/2,cy=y+h/2,along=dx?w:h,across=dx?h:w;
    // A ramp that names a style is a plank of that material (a board propped in a sink, say): no safety edges, darker at its foot.
    const plank=ramp.style!==undefined&&ramp.style!==null,colour=plank?STYLE[ramp.style]??STYLE.wood:null;
    const fill=ctx.createLinearGradient(cx-dx*w/2,cy-dz*h/2,cx+dx*w/2,cy+dz*h/2);
    if(plank){fill.addColorStop(0,shade(colour,-.34));fill.addColorStop(1,shade(colour,.1))}
    else{fill.addColorStop(0,'#8f9b9d');fill.addColorStop(1,'#dfe5e5')}
    ctx.fillStyle=fill;ctx.fillRect(x,y,w,h);
    if(plank){
      // The grain runs up the climb, and a wooden board has its hanging hole near the top.
      const lines=Math.max(1,Math.min(5,Math.floor(across/8)));
      ctx.strokeStyle=dark(colour)?'#ffffff24':'#3a24101f';ctx.lineWidth=1;ctx.beginPath();
      for(let i=1;i<=lines;i++){const t=i/(lines+1);if(dx){ctx.moveTo(x,y+h*t);ctx.lineTo(x+w,y+h*t)}else{ctx.moveTo(x+w*t,y);ctx.lineTo(x+w*t,y+h)}}
      ctx.stroke();
      ctx.strokeStyle=shade(colour,-.5);ctx.lineWidth=1.5;ctx.strokeRect(x,y,w,h);
      const s=this.scale;
      if(ramp.style==='wood'&&.013*s>=2){
        ctx.beginPath();ctx.ellipse(cx+dx*along*.38,cy+dz*along*.38,(dx?.008:.013)*s,(dx?.013:.008)*s,0,0,7);ctx.fillStyle='#2a1a0e';ctx.fill();
      }
    }else{
      ctx.strokeStyle='#4d5a5e';ctx.lineWidth=1;ctx.strokeRect(x,y,w,h);
      // Yellow safety edges run up both sides, as on the ramps in the game.
      ctx.strokeStyle='#e9b824';ctx.lineWidth=Math.min(4,Math.max(2,across*.12));ctx.beginPath();
      const inset=ctx.lineWidth/2;
      if(dx){ctx.moveTo(x,y+inset);ctx.lineTo(x+w,y+inset);ctx.moveTo(x,y+h-inset);ctx.lineTo(x+w,y+h-inset)}
      else{ctx.moveTo(x+inset,y);ctx.lineTo(x+inset,y+h);ctx.moveTo(x+w-inset,y);ctx.lineTo(x+w-inset,y+h)}
      ctx.stroke();
    }
    // Chevrons point uphill.
    const size=Math.min(9,across*.28),count=Math.max(1,Math.min(4,Math.floor(along/26)));
    ctx.strokeStyle=plank&&dark(colour)?'#ffffffcc':'#2e3b40';ctx.lineWidth=2;ctx.lineJoin='round';ctx.beginPath();
    for(let i=0;i<count;i++){
      const t=(i+1)/(count+1)-.5,px=cx+dx*along*t,py=cy+dz*along*t;
      ctx.moveTo(px-dx*size*.5-dz*size,py-dz*size*.5-dx*size);ctx.lineTo(px+dx*size*.5,py+dz*size*.5);ctx.lineTo(px-dx*size*.5+dz*size,py-dz*size*.5+dx*size);
    }
    ctx.stroke();
  }
  #block(block){
    if(KITS.indexOf(block.style)>=FIRST_KITS){this.#thing(block);return}
    if(block.style===ROUND&&Math.max(block.rect[2]-block.rect[0],block.rect[3]-block.rect[1])<=.08){this.#chopstick(block);return}
    const {ctx}=this,[x,y,w,h]=this.#screen(block.rect),colour=STYLE[block.style]??STYLE.steel,lift=Math.min(12,Math.max(2,block.height*this.scale*.16));
    ctx.fillStyle='#0f191d47';ctx.fillRect(x+lift*.6,y+lift,w,h);
    ctx.fillStyle=colour;ctx.fillRect(x,y,w,h);
    if(KITS.includes(block.style)){
      // The appliance's front is the side it faces.
      const [dx,dz]=SIDE[block.facing],t=Math.min(7,Math.max(3,(dx?w:h)*.12));
      ctx.fillStyle=shade(colour,dark(colour)?.3:-.3);
      ctx.fillRect(dx>0?x+w-t:x,dz>0?y+h-t:y,dx?t:w,dz?t:h);
      this.#label(KIT_NAME[block.style]??block.style,x+w/2,y+h/2,w,dark(colour)?'#fff':INK);
    }
    ctx.strokeStyle=shade(colour,.35);ctx.lineWidth=1;ctx.strokeRect(x+1.5,y+1.5,Math.max(0,w-3),Math.max(0,h-3));
    ctx.strokeStyle=shade(colour,-.5);ctx.lineWidth=1.5;ctx.strokeRect(x,y,w,h);
  }
  /**
   * A thing that stands on the counter (one of KITS after the appliances), seen from above in its own frame (front up the screen, as when it faces -z,
   * then turned to its `facing`) over its block's box, drawn faintly, for the box is what hotdogs bump into; then its front, marked as on an appliance.
   */
  #thing(block){
    const {ctx}=this,s=this.scale,[x,y,w,h]=this.#screen(block.rect),kind=block.style,colour=STYLE[kind]??STYLE.steel,edge=shade(colour,-.5);
    const along=block.facing[1]==='z',W=along?w:h,D=along?h:w,n=Math.min(W,D);
    ctx.fillStyle='#0f191d2e';ctx.fillRect(x+2,y+3,w,h);
    ctx.fillStyle=colour+'3d';ctx.fillRect(x,y,w,h);
    const disc=(cx,cy,r,fill,stroke=null,width=1)=>{
      if(!(r>0))return;
      ctx.beginPath();ctx.arc(cx,cy,r,0,7);
      if(fill){ctx.fillStyle=fill;ctx.fill()}
      if(stroke){ctx.strokeStyle=stroke;ctx.lineWidth=width;ctx.stroke()}
    };
    const bar=(x0,y0,x1,y1,width,stroke,cap='round')=>{ctx.beginPath();ctx.moveTo(x0,y0);ctx.lineTo(x1,y1);ctx.strokeStyle=stroke;ctx.lineWidth=width;ctx.lineCap=cap;ctx.stroke();ctx.lineCap='butt'};
    ctx.save();ctx.translate(x+w/2,y+h/2);ctx.rotate(TURN[block.facing]??0);
    switch(kind){
      case 'kettle':{
        // Round body at the back, its lid and knob, the spout out to the front edge and the handle arching over, front to back.
        const r=Math.min(W,D*.8)/2*.94,z=D/2-r-.004*s;
        bar(0,z-r*.7,0,-D/2+.012*s,Math.max(2,r*.3),shade(colour,-.12));
        disc(0,z,r,colour,edge,1.5);disc(0,z,r*.62,BITS.steel,shade(BITS.steel,-.4));disc(0,z,r*.13,BITS.black);
        bar(0,z-r*.62,0,z+r*.62,Math.max(1.5,r*.14),BITS.black);
        break;
      }
      case 'knife-block':{
        // A wooden block leaning back, the handles of five knives out of its top.
        ctx.fillStyle=colour;ctx.fillRect(-W*.47,-D/2,W*.94,D);ctx.fillStyle=shade(colour,-.16);ctx.fillRect(-W*.47,-D*.15,W*.94,D*.6);
        ctx.strokeStyle=edge;ctx.lineWidth=1;ctx.strokeRect(-W*.47,-D/2,W*.94,D);
        const hw=Math.max(2,.016*s),hd=Math.max(3,.03*s);
        ctx.fillStyle=BITS.black;
        [-.36,-.18,0,.18,.36].forEach((k,i)=>ctx.fillRect(k*W*.94-hw/2,-D/2+D*(i%2?.66:.4)-hd/2,hw,hd));
        break;
      }
      case 'canister':{
        // A ceramic tub under a wooden lid with a steel knob.
        const r=n*.48;
        disc(0,0,r,colour,edge,1.5);disc(0,0,r*.8,BITS.walnut);disc(0,0,r*.24,BITS.steel,shade(BITS.steel,-.4));
        break;
      }
      case 'jar':{
        // Glass round a coloured screw lid, with a glint on the glass.
        const r=n*.48;
        disc(0,0,r,colour,BITS.glass,1.5);disc(0,0,r*.64,STYLE.red,shade(STYLE.red,-.4));
        ctx.beginPath();ctx.arc(0,0,r*.83,3.5,4.9);ctx.strokeStyle='#ffffffd9';ctx.lineWidth=Math.max(1,r*.12);ctx.stroke();
        break;
      }
      case 'cereal':{
        // A tall box: the white panel of its front along the front edge, a red band round the top at the back.
        ctx.fillStyle=colour;ctx.fillRect(-W*.485,-D*.47,W*.97,D*.94);
        ctx.fillStyle=BITS.white;ctx.fillRect(-W*.39,-D*.47,W*.78,Math.max(2,D*.26));
        ctx.fillStyle=STYLE.red;ctx.fillRect(-W*.485,D*.2,W*.97,Math.max(1.5,D*.14));
        ctx.strokeStyle=edge;ctx.lineWidth=1.5;ctx.strokeRect(-W*.485,-D*.47,W*.97,D*.94);
        break;
      }
      case 'paper-towel':{
        // A white roll on a wooden stand: the card tube, the spindle's knob, and the loose end hanging at the front.
        const r=n*.46,sheet=Math.max(1.5,.005*s);
        disc(0,0,r*1.08,BITS.beech,shade(BITS.beech,-.4));disc(0,0,r,colour,'#b3ad9c',1);
        disc(0,0,r*.34,BITS.card);disc(0,0,Math.max(1.5,r*.17),shade(BITS.beech,-.1),shade(BITS.beech,-.5));
        ctx.fillStyle=colour;ctx.fillRect(-r*.65,-r-sheet,r*1.3,sheet);ctx.strokeStyle='#b3ad9c';ctx.lineWidth=1;ctx.strokeRect(-r*.65,-r-sheet,r*1.3,sheet);
        break;
      }
      case 'dish-rack':{
        // A frame of coated wire, plates standing across it and two mugs upside down at one end.
        ctx.fillStyle='#ffffff3d';ctx.fillRect(-W/2,-D/2,W,D);
        ctx.strokeStyle=shade(colour,-.4);ctx.lineWidth=Math.max(1,.006*s);ctx.strokeRect(-W/2,-D/2,W,D);
        const count=Math.floor(W/s*.62/.035),radius=Math.min(block.height*.62,D/s*.44)*s;
        for(let i=0;i<count;i++){const px=-W/2+(.03+i*.035)*s;bar(px,-radius,px,radius,Math.max(1.5,.009*s),i%4===3?'#1a8080':'#f4f1e8','butt');bar(px+Math.max(1.5,.009*s)/2,-radius,px+Math.max(1.5,.009*s)/2,radius,1,'#00000040','butt')}
        [-1,1].forEach((side,k)=>disc(W/2-.055*s,side*D*.22,.038*s,BITS.mug[k],shade(BITS.mug[k],-.45)));
        break;
      }
      case 'spice-rack':{
        // A walnut rack, its back panel behind, and a row of little jars with black and white caps.
        ctx.fillStyle=colour;ctx.fillRect(-W/2,-D/2,W,D);
        const back=Math.max(2,.012*s);ctx.fillStyle=shade(colour,-.35);ctx.fillRect(-W/2,D/2-back,W,back);
        const count=Math.floor((W/s-.02)/.042);
        for(let i=0;i<count;i++)disc(-W/2+(.033+i*.042)*s,-back/2,.016*s,BITS.spice[i%BITS.spice.length],i%2?BITS.white:BITS.black,Math.max(1,.005*s));
        ctx.strokeStyle=edge;ctx.lineWidth=1;ctx.strokeRect(-W/2,-D/2,W,D);
        break;
      }
      case 'fruit-bowl':{
        // A bowl heaped with oranges, apples and pears, and a banana over the top.
        const r=n*.5,size=Math.min(r*.36,.04*s);
        disc(0,0,r,colour,edge,1.5);disc(0,0,r*.8,shade(colour,.22));
        for(const [fx,fz,k] of [[-.3,.2,0],[.28,.24,1],[0,-.32,2],[.33,-.18,1],[-.32,-.2,0],[-.05,.02,0]])disc(fx*r,fz*r,size,BITS.fruit[k],shade(BITS.fruit[k],-.45));
        ctx.beginPath();ctx.arc(r*.05,r*.5,r*.5,-2.4,-.75);ctx.strokeStyle=BITS.banana;ctx.lineWidth=Math.max(2,size*.8);ctx.lineCap='round';ctx.stroke();ctx.lineCap='butt';
        break;
      }
      case 'gong':{
        // A wok gong from above: the frame's two chopsticks at the ends and the bar between them, the wok hanging under it, its face to the front (up the
        // screen), its wooden handle out to one side, and the spoon that strikes it leaning on a post.
        const post=Math.max(1.5,.01*s),wok=Math.min(W*.4,D*1.6);
        bar(-W*.45,0,W*.45,0,post,BITS.beech);
        disc(-W*.45,0,Math.max(2,.018*s),BITS.white,shade(BITS.white,-.4));disc(W*.45,0,Math.max(2,.018*s),BITS.white,shade(BITS.white,-.4));
        ctx.beginPath();ctx.ellipse(0,-D*.12,wok,Math.max(2,D*.28),0,0,7);ctx.fillStyle=colour;ctx.fill();ctx.strokeStyle='#000';ctx.lineWidth=1;ctx.stroke();
        bar(wok,-D*.12,wok+.05*s,-D*.12,Math.max(1.5,.012*s),BITS.walnut);
        bar(W*.42,-D*.2,W*.3,-D*.45,Math.max(1.5,.008*s),BITS.beech);
        break;
      }
      case 'griddle':{
        // An electric griddle from above: the grey housing, the dark non-stick plate inside its raised rim, the grease channel along the front, the dial and its light on the front edge (the
        // top, towards -z), the cord out of the back.
        ctx.fillStyle=BITS.steel;ctx.fillRect(-W/2,-D/2,W,D);
        const px=.012*s,py=.018*s;
        ctx.fillStyle='#232729';ctx.fillRect(-W/2+px,-D/2+py,W-2*px,D-py-px);
        ctx.strokeStyle=shade(BITS.steel,-.35);ctx.lineWidth=Math.max(1,.006*s);ctx.strokeRect(-W/2+px,-D/2+py,W-2*px,D-py-px);
        ctx.fillStyle=BITS.black;ctx.fillRect(-W/2+px,-D/2+py,W-2*px,Math.max(1.5,.008*s));
        bar(-W*.3,D/2,-W*.3,D/2+.05*s,Math.max(1.2,.006*s),BITS.black);
        disc(0,-D/2+.009*s,Math.max(2,.011*s),BITS.black,BITS.steel,Math.max(1,.003*s));disc(.05*s,-D/2+.009*s,Math.max(1.5,.005*s),'#e2512e');
        break;
      }
      default:{
        // A tap: its round base, the lever on its side and the gooseneck arching out over the sink in front, to the outlet.
        const reach=.1*s;
        bar(.016*s,0,.034*s,.06*s,Math.max(1.5,.01*s),shade(colour,-.25));
        bar(0,0,0,-2*reach,Math.max(2,.026*s),shade(colour,-.5));bar(0,0,0,-2*reach,Math.max(1,.026*s-2),colour);
        disc(0,0,Math.max(2.5,.03*s),colour,edge,1.2);
        disc(0,-2*reach,Math.max(2,.016*s),colour,edge,1);disc(0,-2*reach,Math.max(1,.009*s),BITS.black);
      }
    }
    ctx.restore();
    // The front, marked as on an appliance, and the block's box, dashed.
    const [dx,dz]=SIDE[block.facing],t=Math.min(4,Math.max(2,(dx?w:h)*.08));
    ctx.fillStyle=shade(colour,-.45);ctx.fillRect(dx>0?x+w-t:x,dz>0?y+h-t:y,dx?t:w,dz?t:h);
    ctx.strokeStyle=edge;ctx.lineWidth=1;ctx.setLineDash([3,2]);ctx.strokeRect(x,y,w,h);ctx.setLineDash([]);
    // Close up, a caption under the box says what it is.
    if(s>=220){ctx.fillStyle=INK;ctx.font=`500 11.5px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='top';ctx.fillText(KIT_NAME[kind]??kind,x+w/2,y+h+3)}
  }
  /** A sink: a dark basin sunk into the counter inside its steel flange, the drain in the middle, labelled with how deep it is. */
  #sink(sink){
    const {ctx}=this,s=this.scale,[x,y,w,h]=this.#screen(sink.rect),rim=Math.max(2,.028*s),cx=x+w/2,cy=y+h/2;
    ctx.fillStyle=SINK.rim;ctx.beginPath();ctx.roundRect(x-rim,y-rim,w+2*rim,h+2*rim,Math.min(rim,8));ctx.fill();
    ctx.strokeStyle=SINK.edge;ctx.lineWidth=1;ctx.stroke();
    const fill=ctx.createRadialGradient(cx,cy,0,cx,cy,Math.max(w,h)*.62);
    fill.addColorStop(0,SINK.light);fill.addColorStop(1,SINK.deep);
    ctx.fillStyle=fill;ctx.fillRect(x,y,w,h);
    // The sides fall away out of the light: a shadow inside the top and left edges.
    const fall=Math.min(w,h)*.07;
    ctx.fillStyle='#00000047';ctx.fillRect(x,y,w,fall);ctx.fillRect(x,y+fall,fall,h-fall);
    ctx.strokeStyle='#0d1315';ctx.lineWidth=1.5;ctx.strokeRect(x,y,w,h);
    const r=.045*s;
    if(r>=2.5){
      ctx.beginPath();ctx.arc(cx,cy,r,0,7);ctx.fillStyle=SINK.drain;ctx.fill();
      ctx.beginPath();ctx.arc(cx,cy,r*.76,0,7);ctx.fillStyle=SINK.hole;ctx.fill();
      ctx.strokeStyle=SINK.drain;ctx.lineWidth=Math.max(1,r*.13);ctx.beginPath();ctx.moveTo(cx-r*.76,cy);ctx.lineTo(cx+r*.76,cy);ctx.moveTo(cx,cy-r*.76);ctx.lineTo(cx,cy+r*.76);ctx.stroke();
    }
    // (At the top: a tap usually stands at the back.)
    if(h>=34)this.#label(`Sink ${sink.depth} m deep`,cx,y+fall+9,w,'#fff');
  }
  /** A pedal bin on the kitchen floor, seen from above and turned by its yaw: the oval steel lid, its hinge behind and the pedal in front. */
  #bin(prop){
    const {ctx}=this,s=this.scale,[hx,hz]=FIXED.bin,x=this.sx(prop.p[0]),y=this.sy(prop.p[1]);
    ctx.fillStyle='#0000005c';ctx.beginPath();ctx.ellipse(x+2,y+4,hx*s,hz*s,-prop.yaw,0,7);ctx.fill();
    ctx.save();ctx.translate(x,y);ctx.rotate(-prop.yaw);
    ctx.fillStyle=BIN.black;ctx.beginPath();ctx.roundRect(-.055*s,-(hz+.065)*s,.11*s,.08*s,Math.max(1,.01*s));ctx.fill();
    const fill=ctx.createLinearGradient(-hx*s,-hz*s,hx*s,hz*s);fill.addColorStop(0,BIN.light);fill.addColorStop(1,BIN.dark);
    ctx.beginPath();ctx.ellipse(0,0,hx*s,hz*s,0,0,7);ctx.fillStyle=fill;ctx.fill();ctx.strokeStyle=BIN.black;ctx.lineWidth=Math.max(1.5,.008*s);ctx.stroke();
    ctx.beginPath();ctx.ellipse(0,0,hx*s*.78,hz*s*.76,0,0,7);ctx.strokeStyle=BIN.ring;ctx.lineWidth=1;ctx.stroke();
    ctx.fillStyle=BIN.black;ctx.fillRect(-.06*s,(hz-.004)*s,.12*s,Math.max(2,.02*s));
    ctx.restore();
    this.#label('Bin',x,y,1.5*Math.min(hx,hz)*s,INK);
  }
  #box(prop){
    const {ctx}=this,[x,y,w,h]=this.#screen(prop.rect),colour=STYLE[prop.style];
    ctx.fillStyle=colour;ctx.fillRect(x,y,w,h);
    ctx.strokeStyle=shade(colour,-.45);ctx.lineWidth=1;
    if(w>12&&h>12){ctx.beginPath();ctx.moveTo(x,y);ctx.lineTo(x+w,y+h);ctx.moveTo(x+w,y);ctx.lineTo(x,y+h);ctx.stroke()}
    ctx.lineWidth=1.5;ctx.setLineDash([4,2]);ctx.strokeRect(x,y,w,h);ctx.setLineDash([]);
  }
  #turret(prop,r){
    const {ctx}=this,x=this.sx(prop.p[0]),y=this.sy(prop.p[1]),[fx,fz]=facing(prop.yaw);
    ctx.strokeStyle='#1d282c';ctx.lineWidth=Math.max(3,r*.45);ctx.lineCap='round';
    ctx.beginPath();ctx.moveTo(x,y);ctx.lineTo(x+fx*r*1.7,y+fz*r*1.7);ctx.stroke();ctx.lineCap='butt';
    ctx.beginPath();ctx.arc(x,y,r,0,7);ctx.fillStyle='#55626a';ctx.fill();ctx.lineWidth=1.5;ctx.stroke();
    ctx.beginPath();ctx.arc(x,y,r*.38,0,7);ctx.fillStyle='#e9b824';ctx.fill();
  }
  /** A loose object seen from above, turned by its yaw: the bowl as rim and base circles, the rest as their footprints. */
  #loose(prop){
    const {ctx}=this,s=this.scale,[hx,hz]=FOOTPRINT[prop.kind]??[.05,.05],colour=LOOSE_COLOR[prop.kind]??'#b9c1c4';
    ctx.save();ctx.translate(this.sx(prop.p[0]),this.sy(prop.p[1]));ctx.rotate(-prop.yaw);
    ctx.lineWidth=1.5;ctx.strokeStyle=shade(colour,-.5);
    if(prop.kind==='bowl'){
      ctx.beginPath();ctx.arc(0,0,hx*s,0,7);ctx.fillStyle=colour;ctx.fill();ctx.stroke();
      ctx.beginPath();ctx.arc(0,0,.065*s,0,7);ctx.fillStyle=shade(colour,-.14);ctx.fill();ctx.lineWidth=1;ctx.stroke();
      ctx.beginPath();ctx.arc(0,0,hx*s*.86,3.5,4.9);ctx.strokeStyle='#ffffffcc';ctx.lineWidth=Math.max(1.5,hx*s*.07);ctx.stroke();
    }else if(['plate','dish','tin','roll','salt','pepper','cup'].includes(prop.kind)){
      // The kitchen's turned pieces, round from above: a plate's rim, a bowl's well, a tin's lid, a roll's core, a shaker's cap, a mug and its handle.
      const r=prop.kind==='cup'?.042:Math.min(hx,hz);
      if(prop.kind==='cup'){ctx.beginPath();ctx.roundRect(.03*s,-.006*s,.037*s,.012*s,2);ctx.fillStyle=colour;ctx.fill();ctx.stroke()}
      ctx.beginPath();ctx.arc(0,0,r*s,0,7);ctx.fillStyle=colour;ctx.fill();ctx.stroke();ctx.lineWidth=1;
      const ring=(f,fill)=>{ctx.beginPath();ctx.arc(0,0,r*s*f,0,7);if(fill){ctx.fillStyle=fill;ctx.fill()}else ctx.stroke()};
      if(prop.kind==='plate'){ctx.strokeStyle='#2a4ea0';ring(.9);ring(.62)}
      if(prop.kind==='dish')ring(.55,'#f3efe4');
      if(prop.kind==='tin')ring(.8);
      if(prop.kind==='roll')ring(.35,'#a0784c');
      if(prop.kind==='cup')ring(.85,'#4a2a14');
      if(prop.kind==='salt'||prop.kind==='pepper')ring(.7,'#c7ccd1');
    }else{
      const soft=prop.kind==='pillow'||prop.kind==='napkin';
      ctx.beginPath();ctx.roundRect(-hx*s,-hz*s,2*hx*s,2*hz*s,soft?Math.min(hx,hz)*s*.45:2);ctx.fillStyle=colour;ctx.fill();ctx.stroke();
      ctx.lineWidth=1;
      // A chair's back is on its far side; a tray has a raised rim; a crate has slats.
      if(prop.kind==='chair'){ctx.fillStyle=shade(colour,-.35);ctx.fillRect(-hx*s,-hz*s,2*hx*s,Math.max(2,hz*s*.3))}
      if(prop.kind==='tray')ctx.strokeRect(-hx*s+3,-hz*s+3,2*hx*s-6,2*hz*s-6);
      if(prop.kind==='crate'){ctx.beginPath();ctx.moveTo(-hx*s,-hz*s);ctx.lineTo(hx*s,hz*s);ctx.moveTo(hx*s,-hz*s);ctx.lineTo(-hx*s,hz*s);ctx.stroke()}
      if(prop.kind==='napkin'){ctx.beginPath();ctx.moveTo(-hx*s,0);ctx.lineTo(hx*s,0);ctx.moveTo(0,-hz*s);ctx.lineTo(0,hz*s);ctx.strokeStyle=shade(colour,.35);ctx.stroke()}
      // A cutting board has its handle slot at one end; a cereal box its front (+z) printed; a bread bin its roll top at the front.
      if(prop.kind==='board'){ctx.beginPath();ctx.roundRect(.162*s,-.05*s,.036*s,.1*s,3);ctx.fillStyle='#2b2118';ctx.fill()}
      if(prop.kind==='cereal'){ctx.fillStyle='#c4161c';ctx.fillRect(-hx*s,hz*s-Math.max(2,hz*s*.4),2*hx*s,Math.max(2,hz*s*.4))}
      if(prop.kind==='breadbox'){ctx.fillStyle=shade(colour,-.2);for(let k=1;k<4;k++)ctx.fillRect(-hx*s,hz*s*(k/4),2*hx*s,1)}
    }
    ctx.restore();
  }
  /** A Forts area: a team's base as a tinted, dashed patch with its name, the forcefield as a hatched blue strip. */
  #zone(zone){
    const {ctx}=this,[x,y,w,h]=this.#screen(zone.rect),field=zone.kind==='field',colour=field?FIELD:TEAM_COLOR[zone.team]??'#8d9a9b';
    ctx.fillStyle=colour+(field?'66':'40');ctx.fillRect(x,y,w,h);
    ctx.save();ctx.beginPath();ctx.rect(x,y,w,h);ctx.clip();
    ctx.strokeStyle=colour+(field?'cc':'59');ctx.lineWidth=field?1.5:2;ctx.beginPath();
    for(let d=-h;d<w;d+=field?6:14){ctx.moveTo(x+d,y+h);ctx.lineTo(x+d+h,y)}
    ctx.stroke();ctx.restore();
    ctx.strokeStyle=field?shade(colour,-.25):shade(colour,-.2);ctx.lineWidth=2;ctx.setLineDash(field?[]:[7,4]);ctx.strokeRect(x,y,w,h);ctx.setLineDash([]);
    // The name is a tab on the area's top edge, outside it, so that it reads whatever stands in the area.
    const text=zoneName(zone);
    ctx.font=`600 12.5px ${FONT}`;
    const tw=ctx.measureText(text).width+10,tx=Math.max(x,Math.min(x+w-tw,x+w/2-tw/2));
    if(w<3||h<3)return;
    ctx.fillStyle=shade(colour,-.35);ctx.beginPath();ctx.roundRect(tx,y-18,tw,18,[4,4,0,0]);ctx.fill();
    ctx.fillStyle='#fff';ctx.textAlign='center';ctx.textBaseline='middle';ctx.fillText(text,tx+tw/2,y-8.5);
  }
  /** The bacon's spot: two wavy rashers on a pale plate, ringed so that it cannot be missed. */
  #bacon(zone,r){
    const {ctx}=this,x=this.sx(zone.p[0]),y=this.sy(zone.p[1]),R=r*1.5;
    ctx.beginPath();ctx.arc(x,y,R,0,7);ctx.fillStyle='#fffaf0';ctx.fill();ctx.strokeStyle=BACON[0];ctx.lineWidth=2;ctx.stroke();
    ctx.beginPath();ctx.arc(x,y,R+4,0,7);ctx.strokeStyle=BACON[0]+'88';ctx.lineWidth=1.5;ctx.setLineDash([4,3]);ctx.stroke();ctx.setLineDash([]);
    ctx.lineCap='round';
    for(const off of [-.36,.36]){
      const wave=()=>{ctx.beginPath();for(let i=0;i<=12;i++){const t=i/12-.5,px=x+t*R*1.45,py=y+off*R+Math.sin(i*1.57)*R*.09;if(i)ctx.lineTo(px,py);else ctx.moveTo(px,py)}ctx.stroke()};
      ctx.strokeStyle=BACON[0];ctx.lineWidth=R*.42;wave();
      ctx.strokeStyle=BACON[1];ctx.lineWidth=R*.12;wave();
    }
    ctx.lineCap='butt';
    if(this.scale>=300){ctx.fillStyle=INK;ctx.font=`600 11.5px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='top';ctx.fillText('Bacon',x,y+R+7)}
  }
  /** A gun pickup is a diamond in the gun's colour with its initial; a box of ammunition is an olive crate with three cartridges in it. */
  #pickup(pickup,r){
    const {ctx}=this,x=this.sx(pickup.p[0]),y=this.sy(pickup.p[1]),colour=weaponColor(pickup.weapon),box=pickup.weapon===AMMO_BOX;
    if(box)this.#ammoBox(x,y,r,colour);
    else{
      ctx.beginPath();ctx.moveTo(x,y-r*1.15);ctx.lineTo(x+r*1.15,y);ctx.lineTo(x,y+r*1.15);ctx.lineTo(x-r*1.15,y);ctx.closePath();
      ctx.fillStyle=colour;ctx.fill();ctx.strokeStyle=shade(colour,-.5);ctx.lineWidth=1.5;ctx.stroke();
      if(r>=8){ctx.fillStyle=dark(colour)?'#fff':INK;ctx.font=`600 ${Math.min(15,r*1.05)}px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='middle';ctx.fillText((WEAPONS[pickup.weapon]??'?')[0],x,y+.5)}
    }
    if(this.scale>=420){ctx.fillStyle=INK;ctx.font=`500 11.5px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='top';ctx.fillText(box?'Ammo':`× ${pickup.ammo}`,x,y+r*1.15+3)}
  }
  #ammoBox(x,y,r,colour){
    const {ctx}=this,s=r*1.05,top=y-s*.82,bottom=y+s*.82;
    ctx.beginPath();ctx.roundRect(x-s,top,2*s,bottom-top,Math.max(1.5,s*.16));
    ctx.fillStyle=colour;ctx.fill();ctx.strokeStyle=shade(colour,-.5);ctx.lineWidth=1.5;ctx.stroke();
    // The lid, a darker band across the top of the crate.
    const lid=top+s*.42;
    ctx.fillStyle=shade(colour,-.28);ctx.fillRect(x-s+1,top+1,2*s-2,lid-top-1);
    ctx.beginPath();ctx.moveTo(x-s,lid);ctx.lineTo(x+s,lid);ctx.lineWidth=1;ctx.stroke();
    if(r<6)return;
    // Three cartridges standing in it: brass cases with copper tips.
    const w=Math.max(1.6,s*.26),tip=s*.26,base=bottom-s*.22,head=lid+s*.16+tip;
    for(const dx of [-.5,0,.5]){
      const cx=x+dx*s;
      ctx.fillStyle=BRASS;ctx.fillRect(cx-w/2,head,w,base-head);
      ctx.beginPath();ctx.moveTo(cx-w/2,head);ctx.quadraticCurveTo(cx-w/2,head-tip,cx,head-tip);ctx.quadraticCurveTo(cx+w/2,head-tip,cx+w/2,head);ctx.closePath();
      ctx.fillStyle=COPPER;ctx.fill();
    }
  }
  #spawn(spawn,r){
    const {ctx}=this,x=this.sx(spawn.p[0]),y=this.sy(spawn.p[1]);
    ctx.beginPath();ctx.arc(x,y,r,0,7);ctx.fillStyle='#5d9447';ctx.fill();ctx.strokeStyle='#27481f';ctx.lineWidth=1.5;ctx.stroke();
    if(r>=8){ctx.fillStyle='#fff';ctx.font=`600 ${Math.min(15,r*1.15)}px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='middle';ctx.fillText('Z',x,y+.5)}
  }
  #start(start,index,r){
    // A hotdog seen from above, pointing where the player will face.
    const {ctx}=this,x=this.sx(start.p[0]),y=this.sy(start.p[1]),[fx,fz]=facing(start.yaw),half=r*1.25;
    ctx.lineCap='round';
    ctx.strokeStyle='#7d5622';ctx.lineWidth=r*1.5+2;ctx.beginPath();ctx.moveTo(x-fx*half,y-fz*half);ctx.lineTo(x+fx*half,y+fz*half);ctx.stroke();
    ctx.strokeStyle='#e0a95a';ctx.lineWidth=r*1.5;ctx.stroke();
    ctx.strokeStyle='#b4472e';ctx.lineWidth=r*.72;ctx.beginPath();ctx.moveTo(x-fx*half*1.25,y-fz*half*1.25);ctx.lineTo(x+fx*half*1.25,y+fz*half*1.25);ctx.stroke();
    ctx.lineCap='butt';
    // Nose arrow, then the player's number in their colour: their team's on a Forts map.
    const tx=x+fx*(half*1.25+r*.95),ty=y+fz*(half*1.25+r*.95),s=Math.max(4,r*.5);
    ctx.fillStyle=INK;ctx.beginPath();ctx.moveTo(tx+fx*s,ty+fz*s);ctx.lineTo(tx-fz*s*.8,ty+fx*s*.8);ctx.lineTo(tx+fz*s*.8,ty-fx*s*.8);ctx.closePath();ctx.fill();
    const bx=x-fx*(half+r*.2),by=y-fz*(half+r*.2),br=Math.max(6.5,r*.62);
    ctx.beginPath();ctx.arc(bx,by,br,0,7);ctx.fillStyle=TEAM_COLOR[start.team]??PLAYER[index%PLAYER.length];ctx.fill();ctx.strokeStyle=INK;ctx.lineWidth=1.2;ctx.stroke();
    if(start.team){ctx.beginPath();ctx.arc(x,y,r*1.9,0,7);ctx.strokeStyle=TEAM_COLOR[start.team]+'cc';ctx.lineWidth=2;ctx.stroke()}
    ctx.fillStyle=start.team==='ketchup'?'#fff':INK;ctx.font=`600 ${Math.round(br*1.35)}px ${FONT}`;ctx.textAlign='center';ctx.textBaseline='middle';ctx.fillText(String(index+1),bx,by+.5);
  }
  #ghost(ghost,r){
    const {ctx}=this;
    ctx.save();ctx.globalAlpha=.62;
    if(ghost.kind==='blocks')this.#block({height:.25,style:'steel',...ghost});
    else if(ghost.kind==='decks')this.#deck({height:.28,style:'metal',...ghost});
    else if(ghost.kind==='ramps')this.#ramp(ghost);
    else if(ghost.kind==='sinks')this.#sink({depth:SINK_DEPTH,...ghost});
    else if(ghost.kind==='props'&&ghost.rect)this.#box({style:'wood',...ghost});
    else if(ghost.kind==='props'&&ghost.loose)this.#loose({kind:ghost.loose,p:ghost.p,yaw:ghost.yaw});
    else if(ghost.kind==='props'&&ghost.bin)this.#bin({kind:'bin',p:ghost.p,yaw:ghost.yaw});
    else if(ghost.kind==='props')this.#turret(ghost,r);
    else if(ghost.kind==='starts')this.#start(ghost,ghost.index,r);
    else if(ghost.kind==='spawns')this.#spawn(ghost,r);
    else if(ghost.kind==='pickups')this.#pickup(ghost,r);
    else if(ghost.kind==='base'||ghost.kind==='field')this.#zone(ghost);
    else if(ghost.kind==='bacon')this.#bacon(ghost,r);
    ctx.restore();
    if(ghost.rect){const [x,y,w,h]=this.#screen(ghost.rect);ctx.strokeStyle=SELECT;ctx.lineWidth=1.5;ctx.setLineDash([5,3]);ctx.strokeRect(x,y,w,h);ctx.setLineDash([])}
  }
}
