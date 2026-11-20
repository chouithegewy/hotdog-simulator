// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// 3D view of a map beside the plan: plain WebGL, rebuilt from the map whenever it changes. It is a preview of the
// layout and its heights, not the game's renderer. Axes match the plan: x right, z towards the viewer, y up.
import {KITS,FIRST_KITS,AMMO_BOX,FLOOR,ROUND,round} from '../tools/network/map.mjs';
import {PRESETS} from '../tools/network/presets.mjs';
import {STYLE,PLAYER,weaponColor,LOOSE_COLOR,TEAM_COLOR,FIELD,BACON,facing} from './view.js';

const rgb=hex=>{const n=parseInt(hex.slice(1),16);return [(n>>16)/255,(n>>8&255)/255,(n&255)/255]};
const scale=(c,k)=>[c[0]*k,c[1]*k,c[2]*k];
const cross=(a,b)=>[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]],unit=v=>{const l=Math.hypot(...v)||1;return v.map(c=>c/l)};
const COLOR=Object.fromEntries(Object.entries(STYLE).map(([name,hex])=>[name,rgb(hex)]));
const SELECT=rgb('#2f7df0'),COUNTER=rgb('#e9e4d4'),YELLOW=rgb('#e9b824'),RAMP=rgb('#cdd4d4'),BUN=rgb('#e0a95a'),SAUSAGE=rgb('#b4472e'),ZOMBIE=rgb('#5d9447'),GUN=rgb('#55626a'),INK=rgb('#18242a'),BRASS=rgb('#e2b04a');
// The kitchen's own things (src/kitchen.rs has their palette): metals, wood, paper, the paints of labels, lids and spices, fruit, the fitted cabinets, the floor.
const KIT={steel:rgb('#a8adb2'),chrome:rgb('#d6dbe0'),black:rgb('#1a1c1e'),ceramic:rgb('#ede8dc'),paper:rgb('#f7f5ef'),card:rgb('#94704a'),beech:rgb('#bd8a54'),
  walnut:rgb('#5a371e'),wire:rgb('#e4e4de'),glass:rgb('#d1eaf2'),basin:rgb('#9aa1a6'),hole:rgb('#0b0d0e'),
  paint:['#c71a12','#f7bd1f','#2154b8','#388c33','#f57514','#6b3d1a','#eddba8','#1a8080','#f7f7f2'].map(rgb),fruit:['#ff850f','#b80d0d','#7ab829','#f5d44d'].map(rgb),
  door:rgb('#5a3220'),panel:rgb('#6e3f26'),carcass:rgb('#2e1a0f'),plinth:rgb('#151515'),floor:rgb('#5f5a51')};
const [RED,PAINT_YELLOW,BLUE,GREEN,ORANGE,BROWN,CREAM,TEAL,WHITE]=[0,1,2,3,4,5,6,7,8];
const SUN=(v=>{const l=Math.hypot(...v);return v.map(c=>c/l)})([.42,.82,.38]),FOV=34*Math.PI/180;
const SIDES={'+x':[1,0],'-x':[-1,0],'+z':[0,1],'-z':[0,-1]};
// The turn about the vertical that brings a counter thing's front from -z round to the side it faces (as the game's level::facing turns it).
const TURN={'-z':0,'+z':Math.PI,'-x':Math.PI/2,'+x':-Math.PI/2};
const VERTEX=`attribute vec3 position;attribute vec3 normal;attribute vec3 color;uniform mat4 mvp;varying vec3 vColor;varying vec3 vNormal;
void main(){vColor=color;vNormal=normal;gl_Position=mvp*vec4(position,1.0);}`;
const FRAGMENT=`precision mediump float;varying vec3 vColor;varying vec3 vNormal;uniform vec3 sun;uniform float alpha;
void main(){vec3 n=normalize(vNormal);float key=max(dot(n,sun),0.0);float sky=0.5+0.5*n.y;gl_FragColor=vec4(vColor*(0.36+0.2*sky+0.56*key),alpha);}`;
// A Forts map's forcefield stands this tall in the preview (in the game nothing gets over it), and is this see-through.
const FIELD_HEIGHT=.5,GLASS=.42;

// Column-major 4×4 matrices, as WebGL expects.
const multiply=(a,b)=>{const out=new Float32Array(16);for(let c=0;c<4;c++)for(let r=0;r<4;r++)out[c*4+r]=a[r]*b[c*4]+a[4+r]*b[c*4+1]+a[8+r]*b[c*4+2]+a[12+r]*b[c*4+3];return out};
const perspective=(fov,aspect,near,far)=>{const f=1/Math.tan(fov/2);return new Float32Array([f/aspect,0,0,0,0,f,0,0,0,0,(far+near)/(near-far),-1,0,0,2*far*near/(near-far),0])};
function lookAt(eye,target){
  const norm=v=>{const l=Math.hypot(...v)||1;return v.map(c=>c/l)},cross=(a,b)=>[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]],dot=(a,b)=>a[0]*b[0]+a[1]*b[1]+a[2]*b[2];
  const z=norm([eye[0]-target[0],eye[1]-target[1],eye[2]-target[2]]),x=norm(cross([0,1,0],z)),y=cross(z,x);
  return new Float32Array([x[0],y[0],z[0],0,x[1],y[1],z[1],0,x[2],y[2],z[2],0,-dot(x,eye),-dot(y,eye),-dot(z,eye),1]);
}

/** Collects lit, coloured triangles: nine numbers per corner (position, normal, colour). */
class Mesh {
  constructor(){this.data=[]}
  face(points,normal,color){
    for(let i=1;i+1<points.length;i++)for(const p of [points[0],points[i],points[i+1]])this.data.push(p[0],p[1],p[2],normal[0],normal[1],normal[2],color[0],color[1],color[2]);
  }
  /** A box; `front` names one side ('+x', '-z', …) painted in its own colour. */
  box(x0,y0,z0,x1,y1,z1,color,front=null,frontColor=null){
    const side=name=>front===name?frontColor:color;
    this.face([[x0,y1,z0],[x0,y1,z1],[x1,y1,z1],[x1,y1,z0]],[0,1,0],color);
    this.face([[x1,y0,z0],[x1,y1,z0],[x1,y1,z1],[x1,y0,z1]],[1,0,0],side('+x'));
    this.face([[x0,y0,z0],[x0,y0,z1],[x0,y1,z1],[x0,y1,z0]],[-1,0,0],side('-x'));
    this.face([[x0,y0,z1],[x1,y0,z1],[x1,y1,z1],[x0,y1,z1]],[0,0,1],side('+z'));
    this.face([[x0,y0,z0],[x0,y1,z0],[x1,y1,z0],[x1,y0,z0]],[0,0,-1],side('-z'));
  }
  /** A ramp: a sloped top over vertical sides, with yellow safety edges along the climb. */
  ramp(rect,base,top,rise){
    const [x0,z0,x1,z1]=rect,[dx,dz]=SIDES[rise];
    const h=(x,z)=>dx?((dx>0?x===x1:x===x0)?top:base):((dz>0?z===z1:z===z0)?top:base);
    const a=[x0,h(x0,z0),z0],b=[x1,h(x1,z0),z0],c=[x1,h(x1,z1),z1],d=[x0,h(x0,z1),z1],foot=p=>[p[0],base,p[2]];
    const run=dx?x1-x0:z1-z0,climb=top-base,length=Math.hypot(run,climb)||1;
    this.face([a,d,c,b],[-dx*climb/length,run/length,-dz*climb/length],RAMP);
    const wall=(p,q,normal)=>this.face([foot(p),foot(q),q,p],normal,dx?(normal[2]?YELLOW:scale(RAMP,.8)):(normal[0]?YELLOW:scale(RAMP,.8)));
    wall(a,b,[0,0,-1]);wall(b,c,[1,0,0]);wall(c,d,[0,0,1]);wall(d,a,[-1,0,0]);
  }
  /** A ramp that names a style: a board `thick` deep laid up the climb, its top the walking surface. */
  plank(rect,base,top,rise,color,thick=.012){
    const [x0,z0,x1,z1]=rect,[dx,dz]=SIDES[rise];
    const h=(x,z)=>dx?((dx>0?x===x1:x===x0)?top:base):((dz>0?z===z1:z===z0)?top:base);
    const a=[x0,h(x0,z0),z0],b=[x1,h(x1,z0),z0],c=[x1,h(x1,z1),z1],d=[x0,h(x0,z1),z1],under=p=>[p[0],p[1]-thick,p[2]];
    const run=dx?x1-x0:z1-z0,climb=top-base,length=Math.hypot(run,climb)||1,normal=[-dx*climb/length,run/length,-dz*climb/length];
    this.face([a,d,c,b],normal,color);
    this.face([under(a),under(b),under(c),under(d)],scale(normal,-1),scale(color,.6));
    for(const [p,q,n] of [[a,b,[0,0,-1]],[b,c,[1,0,0]],[c,d,[0,0,1]],[d,a,[-1,0,0]]])this.face([under(p),under(q),q,p],n,scale(color,.82));
  }
  /** A round bar from a to b (each [x, y, z]), closed at both ends. */
  tube(a,b,radius,color,sides=10){
    const k=unit([b[0]-a[0],b[1]-a[1],b[2]-a[2]]),u=unit(cross(k,Math.abs(k[1])<.9?[0,1,0]:[1,0,0])),v=cross(k,u),ring=[];
    for(let i=0;i<sides;i++){const t=i/sides*2*Math.PI,c=Math.cos(t),s=Math.sin(t);ring.push([u[0]*c+v[0]*s,u[1]*c+v[1]*s,u[2]*c+v[2]*s])}
    const at=(p,d)=>[p[0]+d[0]*radius,p[1]+d[1]*radius,p[2]+d[2]*radius];
    for(let i=0;i<sides;i++){const p=ring[i],q=ring[(i+1)%sides];this.face([at(a,p),at(a,q),at(b,q),at(b,p)],[p[0]+q[0],p[1]+q[1],p[2]+q[2]],color)}
    this.face(ring.map(d=>at(b,d)),k,color);this.face(ring.map(d=>at(a,d)),scale(k,-1),color);
  }
  /** A ball of `radius`, or with radii [rx, ry, rz] an egg along the axes. */
  ball(c,radius,color,sides=10,rings=6){
    const [rx,ry,rz]=Array.isArray(radius)?radius:[radius,radius,radius];
    const dir=(i,j)=>{const t=i/sides*2*Math.PI,p=j/rings*Math.PI-Math.PI/2;return [Math.cos(p)*Math.cos(t),Math.sin(p),Math.cos(p)*Math.sin(t)]};
    for(let j=0;j<rings;j++)for(let i=0;i<sides;i++){
      const m=dir(i+.5,j+.5);
      this.face([dir(i,j),dir(i,j+1),dir(i+1,j+1),dir(i+1,j)].map(d=>[c[0]+d[0]*rx,c[1]+d[1]*ry,c[2]+d[2]*rz]),[m[0]/rx,m[1]/ry,m[2]/rz],color);
    }
  }
  /** A box about its centre along any three unit axes, `half` its half-sizes along them. */
  oriented(c,axes,half,color){
    const [a,b,d]=axes.map((axis,i)=>scale(axis,half[i])),[X,Y,Z]=axes;
    const corner=(i,j,k)=>[c[0]+i*a[0]+j*b[0]+k*d[0],c[1]+i*a[1]+j*b[1]+k*d[1],c[2]+i*a[2]+j*b[2]+k*d[2]];
    this.face([corner(-1,1,-1),corner(-1,1,1),corner(1,1,1),corner(1,1,-1)],Y,color);
    this.face([corner(-1,-1,-1),corner(1,-1,-1),corner(1,-1,1),corner(-1,-1,1)],scale(Y,-1),color);
    this.face([corner(1,-1,-1),corner(1,1,-1),corner(1,1,1),corner(1,-1,1)],X,color);
    this.face([corner(-1,-1,-1),corner(-1,-1,1),corner(-1,1,1),corner(-1,1,-1)],scale(X,-1),color);
    this.face([corner(-1,-1,1),corner(1,-1,1),corner(1,1,1),corner(-1,1,1)],Z,color);
    this.face([corner(-1,-1,-1),corner(-1,1,-1),corner(1,1,-1),corner(1,-1,-1)],scale(Z,-1),color);
  }
  /**
   * A surface of revolution about the vertical through `at`, lit from outside (a closed profile's inside faces in): profile points are [radius, height],
   * the radius stretched by stretch[0] along x and stretch[1] along z (an oval), then turned by yaw.
   */
  revolve(at,profile,color,{sides=20,stretch=[1,1],yaw=0}={}){
    const cos=Math.cos(yaw),sin=Math.sin(yaw),turn=(x,z)=>[x*cos+z*sin,-x*sin+z*cos];
    const p=(r,y,t)=>{const [x,z]=turn(Math.cos(t)*r*stretch[0],Math.sin(t)*r*stretch[1]);return [at[0]+x,at[1]+y,at[2]+z]};
    for(let k=0;k+1<profile.length;k++){
      const [r0,y0]=profile[k],[r1,y1]=profile[k+1],dy=y1-y0,dr=r1-r0;
      if(!dy&&!dr)continue;
      for(let i=0;i<sides;i++){
        const a=i/sides*2*Math.PI,b=(i+1)/sides*2*Math.PI,m=(a+b)/2,[nx,nz]=turn(Math.cos(m)*dy/stretch[0],Math.sin(m)*dy/stretch[1]);
        this.face([p(r0,y0,a),p(r0,y0,b),p(r1,y1,b),p(r1,y1,a)],[nx,-dr,nz],color);
      }
    }
  }
  /** An upright prism standing in for a cylinder. */
  post(x,z,y0,y1,radius,color,cap=color,sides=12){
    const ring=[];
    for(let i=0;i<sides;i++){const t=i/sides*2*Math.PI;ring.push([Math.cos(t),Math.sin(t)])}
    for(let i=0;i<sides;i++){
      const p=ring[i],q=ring[(i+1)%sides];
      this.face([[x+p[0]*radius,y0,z+p[1]*radius],[x+q[0]*radius,y0,z+q[1]*radius],[x+q[0]*radius,y1,z+q[1]*radius],[x+p[0]*radius,y1,z+p[1]*radius]],[(p[0]+q[0])/2,0,(p[1]+q[1])/2],color);
    }
    this.face(ring.map(p=>[x+p[0]*radius,y1,z+p[1]*radius]),[0,1,0],cap);
  }
  /** A box turned about the vertical axis; centre and half-sizes in the object's own frame at `at`. */
  turned(at,yaw,centre,half,color){
    const cos=Math.cos(yaw),sin=Math.sin(yaw),world=(x,y,z)=>[at[0]+x*cos+z*sin,at[1]+y,at[2]-x*sin+z*cos];
    const [cx,cy,cz]=centre,[hx,hy,hz]=half,corner=(i,j,k)=>world(cx+i*hx,cy+j*hy,cz+k*hz),turn=(x,z)=>[x*cos+z*sin,0,-x*sin+z*cos];
    this.face([corner(-1,1,-1),corner(-1,1,1),corner(1,1,1),corner(1,1,-1)],[0,1,0],color);
    this.face([corner(1,-1,-1),corner(1,1,-1),corner(1,1,1),corner(1,-1,1)],turn(1,0),color);
    this.face([corner(-1,-1,-1),corner(-1,-1,1),corner(-1,1,1),corner(-1,1,-1)],turn(-1,0),color);
    this.face([corner(-1,-1,1),corner(1,-1,1),corner(1,1,1),corner(-1,1,1)],turn(0,1),color);
    this.face([corner(-1,-1,-1),corner(-1,1,-1),corner(1,1,-1),corner(1,-1,-1)],turn(0,-1),color);
  }
  /** A surface of revolution about the vertical through `at`: profile points are [radius, height]. */
  lathe(at,profile,color,sides=24){
    for(let k=0;k+1<profile.length;k++){
      const [r0,y0]=profile[k],[r1,y1]=profile[k+1];
      for(let i=0;i<sides;i++){
        const a=i/sides*2*Math.PI,b=(i+1)/sides*2*Math.PI,m=(a+b)/2,p=(r,y,t)=>[at[0]+Math.cos(t)*r,at[1]+y,at[2]+Math.sin(t)*r];
        // Seen from above, the inside of a bowl faces up and inwards.
        this.face([p(r0,y0,a),p(r0,y0,b),p(r1,y1,b),p(r1,y1,a)],[-Math.cos(m)*(y1-y0),r1-r0+1e-6,-Math.sin(m)*(y1-y0)],color);
      }
    }
  }
  /** A floating diamond, for pickups. */
  gem(x,y,z,radius,color){
    const top=[x,y+radius*1.3,z],bottom=[x,y-radius*1.3,z],ring=[[1,0],[0,1],[-1,0],[0,-1]].map(p=>[x+p[0]*radius,y,z+p[1]*radius]);
    for(let i=0;i<4;i++){
      const p=ring[i],q=ring[(i+1)%4],n=[(p[0]+q[0])/2-x,radius*.55,(p[2]+q[2])/2-z];
      this.face([p,q,top],n,color);this.face([q,p,bottom],[n[0],-n[1],n[2]],scale(color,.8));
    }
  }
}

/** Height of the floor at a point: the highest deck or ramp under it, or a sink's floor (below 0) where there is neither, like Level.height in the map library. */
function floorAt(map,x,z){
  let h=0;
  for(const sink of map.sinks??[]){const r=sink.rect;if(x>=r[0]&&x<=r[2]&&z>=r[1]&&z<=r[3])h=-sink.depth}
  for(const deck of map.decks){const r=deck.rect;if(deck.height>h&&x>=r[0]&&x<=r[2]&&z>=r[1]&&z<=r[3]&&(deck.style!==ROUND||round(r,x,z)))h=deck.height}
  for(const ramp of map.ramps){
    const r=ramp.rect;
    if(x<r[0]||x>r[2]||z<r[1]||z>r[3])continue;
    const t=ramp.rise[1]==='z'?(z-r[1])/(r[3]-r[1]):(x-r[0])/(r[2]-r[0]),v=ramp.base+(ramp.rise[0]==='+'?t:1-t)*(ramp.height-ramp.base);
    if(v>h)h=v;
  }
  return h;
}

/** The counter top as rectangles [min x, min z, max x, max z]: the counter with a hole cut for each sink, as src/kitchen.rs cuts it. */
function counterTop(map){
  let pieces=[map.bounds];
  for(const {rect:[a,b,c,d]} of map.sinks??[]){
    const cut=[];
    for(const r of pieces){
      if(a>=r[2]||c<=r[0]||b>=r[3]||d<=r[1]){cut.push(r);continue}
      // What is left of the piece round the hole: the strips before and after it along z, then beside it along x.
      const lo=Math.max(b,r[1]),hi=Math.min(d,r[3]);
      for(const piece of [[r[0],r[1],r[2],lo],[r[0],hi,r[2],r[3]],[r[0],lo,Math.max(a,r[0]),hi],[Math.min(c,r[2]),lo,r[2],hi]])if(piece[2]-piece[0]>1e-4&&piece[3]-piece[1]>1e-4)cut.push(piece);
    }
    pieces=cut;
  }
  return pieces;
}
/** A sink's basin: a steel floor `depth` down and four sides (standing a hair inside the hole, clear of the counter's own edges), the drain, and the flange round the hole. */
function basin(mesh,{rect:[x0,z0,x1,z1],depth}){
  const t=.012,e=.0008,low=-depth,cx=(x0+x1)/2,cz=(z0+z1)/2,rim=.028,lip=.003;
  mesh.box(x0-t,low-t,z0-t,x1+t,low,z1+t,scale(KIT.basin,.92));
  mesh.box(x0-t,low,z0-t,x0+e,-.001,z1+t,KIT.basin);mesh.box(x1-e,low,z0-t,x1+t,-.001,z1+t,KIT.basin);
  mesh.box(x0,low,z0-t,x1,-.001,z0+e,KIT.basin);mesh.box(x0,low,z1-e,x1,-.001,z1+t,KIT.basin);
  mesh.post(cx,cz,low,low+.003,.045,KIT.steel);mesh.post(cx,cz,low,low+.0035,.034,KIT.hole);
  mesh.box(cx-.033,low,cz-.003,cx+.033,low+.004,cz+.003,KIT.steel);mesh.box(cx-.003,low,cz-.033,cx+.003,low+.004,cz+.033,KIT.steel);
  mesh.box(x0-rim,0,z0-rim,x1+rim,lip,z0,KIT.steel);mesh.box(x0-rim,0,z1,x1+rim,lip,z1+rim,KIT.steel);
  mesh.box(x0-rim,0,z0,x0,lip,z1,KIT.steel);mesh.box(x1,0,z0,x1+rim,lip,z1,KIT.steel);
}
/** A small number from a block's place, so that like things differ (a jar's contents, a box's colours): the game's `kitchen::seed`, as its web build works it out. */
const seed=rect=>{const f=Math.fround,v=Math.abs(f(f(f(f(rect[0])*731)+f(f(rect[1])*377))+f(f(rect[2])*113)));return (Math.imul(Math.trunc(v),2654435761)>>>0)>>>7};
/**
 * Builds in a counter thing's own frame, as the game does: its front towards -z, centred on its block's box on the counter top, then turned to its `facing`.
 * w is its width across the front, d its depth behind it; positions are [x, y, z] in that frame.
 */
function frame(mesh,rect,side){
  const along=side[1]==='z',yaw=TURN[side]??0,cos=Math.cos(yaw),sin=Math.sin(yaw),cx=(rect[0]+rect[2])/2,cz=(rect[1]+rect[3])/2;
  const p=([x,y,z])=>[cx+x*cos+z*sin,y,cz-x*sin+z*cos],v=([x,y,z])=>[x*cos+z*sin,y,-x*sin+z*cos];
  return {w:along?rect[2]-rect[0]:rect[3]-rect[1],d:along?rect[3]-rect[1]:rect[2]-rect[0],
    box:(c,half,color)=>mesh.turned([cx,0,cz],yaw,c,half,color),
    post:(x,y,z,radius,height,color,cap)=>{const [px,,pz]=p([x,0,z]);mesh.post(px,pz,y,y+height,radius,color,cap)},
    tube:(a,b,radius,color,sides)=>mesh.tube(p(a),p(b),radius,color,sides),
    ball:(c,radius,color)=>mesh.ball(p(c),radius,color),
    oriented:(c,axes,half,color)=>mesh.oriented(p(c),axes.map(v),half,color),
    revolve:(c,profile,color)=>mesh.revolve(p(c),profile,color,{yaw})};
}
/** Half a ring of tube in a frame's y-z plane, centred on `c` and standing on its two ends `radius` either side of it along z. */
function arch(f,[x,y,z],radius,thick,color,steps=10){
  let last=null;
  for(let i=0;i<=steps;i++){const t=i/steps*Math.PI,point=[x,y+Math.sin(t)*radius,z-Math.cos(t)*radius];if(last)f.tube(last,point,thick,color,6);last=point}
}
/** A thing that stands on the counter (one of KITS after the appliances), shaped as src/kitchen.rs shapes it in the game; its glass goes into `glass`. */
function thing(mesh,glass,block){
  const f=frame(mesh,block.rect,block.facing),{w,d}=f,h=block.height,n=seed(block.rect),P=KIT.paint;
  switch(block.style){
    case 'kettle':{
      // Round enamel body at the back, a shoulder up to a steel lid and knob, the spout out to the front edge and the handle arching over.
      const r=Math.min(w,d*.8)/2*.94,z=d/2-r-.004,enamel=P[[RED,TEAL,CREAM,BLUE][n%4]],span=r*.55,top=h*.98-span*1.13;
      f.post(0,0,z,r*.94,.03*h,KIT.steel);f.post(0,.03*h,z,r,.52*h,enamel);
      f.revolve([0,.55*h,z],[[r,0],[r*.6,.14*h],[0,.14*h]],enamel);
      f.post(0,.69*h,z,r*.62,.025*h,KIT.steel);f.ball([0,.73*h,z],r*.12,KIT.black);
      f.tube([0,.22*h,z-r*.8],[0,.58*h,-d/2+.012],r*.16,enamel,8);
      for(const s of [-1,1])f.tube([0,.64*h,z+s*span],[0,top,z+s*span],.006,KIT.black,6);
      arch(f,[0,top,z],span,Math.max(.004,span*.13),KIT.black);
      break;
    }
    case 'knife-block':{
      // A wooden block leaning back about its bottom front edge, on a foot along the counter, with the handles of five knives out of its top.
      const lean=.32,c=Math.cos(lean),s=Math.sin(lean),bw=w*.94,bh=h*.78,bd=d*.62,pz=-d/2+.01,axes=[[1,0,0],[0,c,s],[0,-s,c]];
      const place=([x,y,z])=>[x,y*c-(z-pz)*s,pz+y*s+(z-pz)*c];
      f.oriented(place([0,bh/2,pz+bd/2]),axes,[bw/2,bh/2,bd/2],KIT.beech);
      f.box([0,.006,0],[bw/2,.006,d*.48],KIT.beech);
      [-.36,-.18,0,.18,.36].forEach((x,i)=>{
        const z=pz+bd*(i%2?.62:.3);
        f.oriented(place([x*bw,bh+.03,z]),axes,[.008,.0375,.013],KIT.black);
        f.oriented(place([x*bw,bh+.004,z]),axes,[.009,.004,.014],KIT.steel);
      });
      break;
    }
    case 'canister':{
      // A lidded tub, ceramic or steel, with a coloured label band.
      const r=Math.min(w,d)*.48,steel=n%3===0;
      f.post(0,0,0,r,.86*h,steel?KIT.steel:KIT.ceramic);f.post(0,.32*h,0,r*1.012,.2*h,P[[BLUE,RED,GREEN,BROWN,TEAL][n%5]]);
      f.post(0,.86*h,0,r*1.03,.1*h,steel?KIT.black:KIT.walnut);f.ball([0,.97*h,0],[r*.36,.05*h,r*.36],KIT.steel);
      break;
    }
    case 'jar':{
      // A glass jar of pasta, lentils, beans or rice with a screw lid.
      const r=Math.min(w,d)*.48;
      f.post(0,.006,0,r*.9,(.45+.08*(n%4))*h,P[[PAINT_YELLOW,ORANGE,BROWN,WHITE,RED][n%5]]);
      frame(glass,block.rect,block.facing).post(0,0,0,r,.86*h,KIT.glass);
      f.post(0,.86*h,0,r*.96,.13*h,[P[RED],KIT.steel,P[PAINT_YELLOW],KIT.black][n%4]);
      break;
    }
    case 'cereal':{
      // A tall box in its colour, a band at the top of its front, and a white panel with a bowl of flakes on it.
      const [main,band,bowl]=[[RED,PAINT_YELLOW,BLUE],[PAINT_YELLOW,RED,BLUE],[BLUE,PAINT_YELLOW,RED],[GREEN,ORANGE,BLUE]][n%4],face=-d*.47-.0015;
      f.box([0,h/2,0],[w*.485,h/2,d*.47],P[main]);
      f.box([0,h*.86,face],[w*.485,h*.075,.001],P[band]);f.box([0,h*.86,face-.001],[w*.3,h*.04,.001],P[WHITE]);
      f.box([0,h*.42,face],[w*.39,h*.22,.001],P[WHITE]);f.box([0,h*.34,face-.002],[w*.28,h*.06,.001],P[bowl]);
      for(const [x,y] of [[-.14,.43],[0,.46],[.13,.42],[-.06,.5],[.07,.5]])f.box([x*w,y*h,face-.003],[.011,.008,.001],P[ORANGE]);
      break;
    }
    case 'paper-towel':{
      // A roll of paper towels on a wooden stand, the spindle through its card tube, and the end of the roll hanging loose at the front.
      const r=Math.min(w,d)*.46;
      f.post(0,0,0,r*1.08,.022,KIT.beech);f.post(0,.024,0,r,h*.86,KIT.paper);f.post(0,.024+h*.86,0,r*.34,.002,KIT.card);
      f.post(0,.02,0,.007,h*.97-.02,KIT.steel);f.ball([0,h*.97,0],.013,KIT.beech);
      f.box([.01,h*.42,-r-.0015],[r*.65,h*.3,.001],KIT.paper);
      break;
    }
    case 'dish-rack':{
      // A drip tray, a frame of coated wire, plates standing across it and two mugs upside down at the end.
      const top=h*.6,e=.006,radius=Math.min(h*.62,d*.44),count=Math.floor(w*.62/.035),axis=[-Math.cos(.12),Math.sin(.12),0];
      f.box([0,.006,0],[w/2,.006,d/2],KIT.wire);
      for(const x of [-w/2+e,w/2-e])for(const z of [-d/2+e,d/2-e])f.box([x,top/2,z],[.0025,top/2,.0025],KIT.wire);
      for(const z of [-d/2+e,d/2-e]){f.box([0,top,z],[w/2,.0025,.0025],KIT.wire);f.box([0,top*.45,z],[w/2,.002,.002],KIT.wire)}
      for(const x of [-w/2+e,w/2-e])f.box([x,top,0],[.0025,.0025,d/2],KIT.wire);
      for(let i=0;i<count;i++){
        const c=[-w/2+.03+i*.035,.014+radius,0];
        f.tube(c.map((v,j)=>v-axis[j]*.0035),c.map((v,j)=>v+axis[j]*.0035),radius,(i+n)%4===3?P[TEAL]:KIT.ceramic,16);
      }
      for(const [k,z] of [[0,-d*.22],[1,d*.22]])f.post(w/2-.055,.012,z,.038,Math.min(h*.62,.095),k?KIT.ceramic:P[[RED,BLUE,PAINT_YELLOW][n%3]]);
      break;
    }
    case 'spice-rack':{
      // A walnut rack: a back, two shelves and two sides, a rail across the front of each shelf, and rows of little jars with black and white caps.
      const count=Math.floor((w-.02)/.042),tall=Math.min(h*.5-.02,.075);
      f.box([0,h/2,d/2-.006],[w/2,h/2,.006],KIT.walnut);
      for(const y of [.006,h*.5])f.box([0,y,0],[w/2,.006,d/2],KIT.walnut);
      for(const x of [-w/2+.006,w/2-.006])f.box([x,h/2,0],[.006,h/2,d/2],KIT.walnut);
      for(const y of [h*.16,h*.66])f.tube([-w/2,y,-d/2+.006],[w/2,y,-d/2+.006],.003,KIT.steel,6);
      for(let shelf=0;shelf<2;shelf++){
        const y=shelf?h*.5+.006:.012;
        for(let i=0;i<count;i++){
          const x=-w/2+.033+i*.042;
          f.post(x,y,0,.016,tall*.78,P[[RED,GREEN,BROWN,PAINT_YELLOW,ORANGE,CREAM,RED,GREEN][(i+shelf*3+n)%8]]);
          f.post(x,y+tall*.78,0,.017,tall*.2,(i+shelf)%2?P[WHITE]:KIT.black);
        }
      }
      break;
    }
    case 'fruit-bowl':{
      // A ceramic bowl heaped with oranges, apples and pears, and a banana over the top.
      const r=Math.min(w,d)*.5,deep=h*.55,size=Math.min(r*.36,.04),y=Math.min(deep+size*.9,h-.016);
      f.revolve([0,0,0],[[0,0],[.24,0],[.27,.06],[.42,.42],[.5,1],[.47,1],[.39,.45],[.24,.1],[0,.1]].map(([a,b])=>[a*2*r,b*deep]),[KIT.ceramic,P[TEAL],P[BLUE]][n%3]);
      [[-.3,.2,0],[.28,.24,1],[0,-.32,2],[-.05,.02,0],[.33,-.18,1],[-.32,-.2,0]].forEach(([x,z,k],i)=>f.ball([x*r,Math.min(i===3?deep+size*.6:deep*.55+size*.6,h-size),z*r],size,KIT.fruit[k]));
      for(const [x,turn] of [[-r*.12,.25],[r*.18,-.25]]){
        const dir=[-Math.cos(turn)*Math.cos(.5),-Math.sin(turn),Math.cos(turn)*Math.sin(.5)],c=[x,y,r*.05];
        f.tube(c.map((v,j)=>v-dir[j]*.035),c.map((v,j)=>v+dir[j]*.035),.015,KIT.fruit[3],8);
      }
      break;
    }
    case 'gong':{
      // A wok gong: two long cooking chopsticks standing in white holders at the ends, a third across their tops, the wok hanging from it on two strings with its
      // face to the front, its wooden handle out to the side, and the wooden spoon that strikes it leaning on a post.
      const bamboo=COLOR.bamboo,wok=Math.min(w*.36,h*.34),y=h*.5,string=rgb('#d8cfb8');
      for(const x of [-1,1]){f.post(x*w*.44,0,0,.02,.035,KIT.ceramic);f.post(x*w*.44,.035,0,.006,h-.035,bamboo)}
      f.tube([-w*.47,h*.95,0],[w*.47,h*.95,0],.005,bamboo,6);
      for(const x of [-1,1])f.tube([x*wok*.55,h*.95,0],[x*wok*.55,y+wok*.8,0],.0015,string,4);
      f.tube([0,y,.012],[0,y,-.008],wok,KIT.black,20);f.tube([0,y,-.008],[0,y,-.014],wok*.55,scale(KIT.black,1.6),16);
      f.tube([wok*.95,y,0],[wok*.95+.07,y+.01,0],.008,KIT.walnut,6);
      f.tube([w*.4,0,-.03],[w*.33,h*.4,-.02],.005,KIT.beech,6);f.ball([w*.33,h*.4,-.02],.016,KIT.beech);
      break;
    }
    case 'griddle':{
      // An electric griddle: four short feet, a grey housing, a dark non-stick plate with a raised rim round its sides and back and a grease channel along the front, the
      // drip tray's lip, two cool handles, the thermostat dial and its light on the front, and the cord out of the back.
      const foot=h*.2,body=h*.7,plate=scale(KIT.black,1.45);
      for(const [x,z] of [[-1,-1],[1,-1],[-1,1],[1,1]])f.post(x*(w/2-.025),0,z*(d/2-.025),.009,foot,KIT.black);
      f.box([0,foot+(body-foot)/2,0],[w/2-.004,(body-foot)/2,d/2-.004],KIT.steel);
      f.box([0,body+h*.085,0],[w/2-.012,h*.085,d/2-.02],plate);
      const rim=h*.13;
      for(const x of [-1,1])f.box([x*(w/2-.016),h-rim/2,0],[.004,rim/2,d/2-.012],KIT.steel);
      f.box([0,h-rim/2,d/2-.016],[w/2-.012,rim/2,.004],KIT.steel);
      f.box([0,body+h*.17,-d/2+.022],[w/2-.014,h*.012,.007],KIT.black);
      f.box([0,body-h*.08,-d/2+.012],[w*.34,h*.05,.014],KIT.chrome);
      for(const x of [-1,1])f.box([x*(w/2-.012),body-h*.1,0],[.012,h*.12,.05],KIT.black);
      f.box([0,h*.38,-d/2-.001],[.055,h*.2,.002],KIT.black);
      f.tube([0,h*.38,-d/2],[0,h*.38,-d/2-.016],.011,KIT.black,12);f.tube([0,h*.38,-d/2-.016],[.004,h*.38+.009,-d/2-.017],.0025,KIT.chrome,6);
      f.ball([.05,h*.38,-d/2-.003],.005,rgb('#e2512e'));
      f.tube([-w*.3,h*.3,d/2],[-w*.3,h*.25,d/2+.07],.0035,KIT.black,6);f.tube([-w*.3,h*.25,d/2+.07],[-w*.3-.06,h*.1,d/2+.15],.0035,KIT.black,6);
      break;
    }
    default:{
      // A chrome tap: a round base, a column, a gooseneck arching out over the sink in front to its outlet, and a lever on its side.
      const reach=.1,top=Math.max(.04,h-reach-.013);
      f.post(0,0,0,.03,.022,KIT.chrome);f.post(0,.02,0,.013,top-.02,KIT.chrome);
      arch(f,[0,top,-reach],reach,.013,KIT.chrome,12);
      f.post(0,top-.03,-2*reach,.016,.03,KIT.chrome);
      f.tube([.008,.1,0],[.028,.1,0],.012,KIT.chrome,8);f.tube([.028,.1,0],[.046,.135,.06],.005,KIT.chrome,6);
    }
  }
}
/** A wooden plank's hanging hole, near its top: a dark slot on its surface. */
function hangingHole(mesh,{rect:[x0,z0,x1,z1],base,height,rise}){
  const [dx,dz]=SIDES[rise],run=dx?x1-x0:z1-z0,climb=height-base,length=Math.hypot(run,climb)||1,cx=(x0+x1)/2,cz=(z0+z1)/2;
  const corners=[[-.013,-.008],[.013,-.008],[.013,.008],[-.013,.008]].map(([across,o])=>{
    const along=.38*run+o;
    return [cx+dx*along+(dx?0:across),base+climb*(.5+along/run)+.0006,cz+dz*along+(dx?across:0)];
  });
  mesh.face(corners,[-dx*climb/length,run/length,-dz*climb/length],KIT.hole);
}
/** A pedal bin standing on the kitchen floor (src/kitchen.rs `bin`): an oval of brushed steel on a black base, a lid, its hinge behind and the pedal in front. */
function bin(mesh,{p:[x,z],yaw}){
  const at=[x,-FLOOR,z],w=.34,d=.3,h=.6;
  const oval=(y0,height,grow,color)=>mesh.revolve(at,[[0,y0],[.5,y0],[.5,y0+height],[0,y0+height]],color,{sides:24,stretch:[w+grow,d+grow],yaw});
  oval(0,.03,-.012,KIT.black);oval(.03,h-.06,0,KIT.steel);oval(h-.03,.012,.006,KIT.black);oval(h-.018,.022,.008,KIT.steel);
  mesh.revolve(at,[[.48,h+.004],[.36,h+.011],[0,h+.014]],scale(KIT.steel,1.08),{sides:24,stretch:[w,d],yaw});
  mesh.turned(at,yaw,[0,h-.02,d/2+.006],[.06,.015,.009],KIT.black);
  mesh.turned(at,yaw,[0,.018,-d/2-.03],[.055,.007,.035],KIT.black);
  mesh.turned(at,yaw,[0,.03,-d/2+.01],[.025,.015,.02],KIT.black);
}
/**
 * Fitted cabinets under the counter, down to the kitchen floor (src/kitchen.rs `cabinets`): the carcass over a recessed black plinth, a panel at each end,
 * and down the front a unit about every 0.6 m, a drawer over a door (or a pair of doors, under a sink), each with its handle.
 */
function cabinets(mesh,map){
  const [x0,z0,x1,z1]=map.bounds,floor=-FLOOR,kick=.1,top=-.05,face=z0+.004,front=face-.018,gap=.004,drawer=.15,side=.018;
  // The carcass is its four sides, open at the top, where the counter covers it and a sink's basin hangs into it.
  const [c0,c1,b0]=[x0+side,x1-side,floor+kick];
  mesh.face([[c0,b0,face],[c0,top,face],[c1,top,face],[c1,b0,face]],[0,0,-1],KIT.carcass);
  mesh.face([[c0,b0,z1],[c1,b0,z1],[c1,top,z1],[c0,top,z1]],[0,0,1],KIT.carcass);
  mesh.box(x0+.05,floor,face+.05,x1-.05,floor+kick,z1-.02,KIT.plinth);
  for(const [a,b,c,e] of [[x0,x0+side,x0-.002,x0],[x1-side,x1,x1,x1+.002]]){
    mesh.box(a,floor+kick,z0,b,top,z1,KIT.door);
    mesh.box(c,floor+kick+.07,z0+.07,e,top-.07,z1-.07,KIT.panel);
  }
  const units=Math.max(1,Math.round((x1-x0)/.6)),pitch=(x1-x0)/units;
  for(let i=0;i<units;i++){
    const a=x0+pitch*i,b=a+pitch,mid=(a+b)/2,split=top-drawer,low=floor+kick+gap/2,high=split-gap/2;
    const sink=(map.sinks??[]).some(s=>s.rect[0]<b&&s.rect[2]>a);
    mesh.box(a+gap/2,split+gap/2,front,b-gap/2,top-gap/2,face,KIT.door);
    if(!sink)mesh.box(mid-.08,(split+top)/2-.006,front-.012,mid+.08,(split+top)/2+.006,front,KIT.steel);
    for(const [l,r,handle] of sink||pitch>.75?[[a,mid,1],[mid,b,-1]]:[[a,b,i%2?-1:1]]){
      mesh.box(l+gap/2,low,front,r-gap/2,high,face,KIT.door);
      // A shaker door: an inset panel a shade lighter, and a bar handle near the edge it opens from.
      mesh.box(l+gap/2+.055,low+.055,front-.002,r-gap/2-.055,high-.055,front,KIT.panel);
      const edge=handle>0?r-.045:l+.045;
      mesh.box(edge-.006,high-.185,front-.014,edge+.006,high-.055,front,KIT.steel);
    }
  }
}

export class Solid {
  /** Returns null when the browser has no WebGL. */
  static create(canvas){
    const gl=canvas.getContext('webgl',{antialias:true,alpha:false})??canvas.getContext('experimental-webgl');
    return gl?new Solid(canvas,gl):null;
  }
  constructor(canvas,gl){
    this.canvas=canvas;this.gl=gl;this.count=0;this.lines=0;this.glass=0;
    this.yaw=-.45;this.pitch=.82;this.distance=4;this.target=[0,.1,3];
    const shader=(type,source)=>{const s=gl.createShader(type);gl.shaderSource(s,source);gl.compileShader(s);if(!gl.getShaderParameter(s,gl.COMPILE_STATUS))throw new Error(gl.getShaderInfoLog(s));return s};
    const program=this.program=gl.createProgram();
    gl.attachShader(program,shader(gl.VERTEX_SHADER,VERTEX));gl.attachShader(program,shader(gl.FRAGMENT_SHADER,FRAGMENT));gl.linkProgram(program);
    if(!gl.getProgramParameter(program,gl.LINK_STATUS))throw new Error(gl.getProgramInfoLog(program));
    this.attributes=['position','normal','color'].map(name=>gl.getAttribLocation(program,name));
    this.mvp=gl.getUniformLocation(program,'mvp');this.sun=gl.getUniformLocation(program,'sun');this.alpha=gl.getUniformLocation(program,'alpha');
    this.solidBuffer=gl.createBuffer();this.lineBuffer=gl.createBuffer();this.glassBuffer=gl.createBuffer();
    this.#controls();
  }
  resize(){
    const box=this.canvas.getBoundingClientRect(),dpr=window.devicePixelRatio||1;
    this.canvas.width=Math.max(1,Math.round(box.width*dpr));this.canvas.height=Math.max(1,Math.round(box.height*dpr));
  }
  /** Points the camera at the whole counter. */
  frame(bounds){
    const aspect=this.canvas.width/this.canvas.height||1,halfX=(bounds[2]-bounds[0])/2,halfZ=(bounds[3]-bounds[1])/2,half=Math.tan(FOV/2);
    this.target=[(bounds[0]+bounds[2])/2,.08,(bounds[1]+bounds[3])/2];
    this.distance=1.18*Math.max(halfX/(half*aspect),(halfZ+.35)/half*.8)+halfZ;
    this.yaw=-.45;this.pitch=.82;
  }
  /** Rebuilds the scene from a map; objects whose ids are in `selection` are tinted. */
  setScene(map,selection){
    const mesh=new Mesh,chosen=new Set(selection),b=map.bounds;
    const tint=(color,item)=>chosen.has(item.id)?[color[0]*.45+SELECT[0]*.55,color[1]*.45+SELECT[1]*.55,color[2]*.45+SELECT[2]*.55]:color;
    // Tints what was added to the mesh since `from` when the item is selected.
    const paint=(from,item)=>{if(chosen.has(item.id))for(let i=from;i<mesh.data.length;i+=9)for(let k=0;k<3;k++)mesh.data[i+6+k]=mesh.data[i+6+k]*.45+SELECT[k]*.55};
    // The counter top, with a hole where each sink is sunk into it.
    for(const [x0,z0,x1,z1] of counterTop(map))mesh.box(x0,-.05,z0,x1,0,z1,COUNTER);
    for(const sink of map.sinks??[]){const from=mesh.data.length;basin(mesh,sink);paint(from,sink)}
    // What stands on the kitchen floor, 0.9 m below the counter top: the fitted cabinets under the counter, the bins beside it, and the floor itself round them.
    const bins=map.props.filter(prop=>prop.kind==='bin');
    if(map.cabinets||bins.length){
      let [x0,z0,x1,z1]=b;
      for(const {p:[x,z]} of bins){x0=Math.min(x0,x-.25);z0=Math.min(z0,z-.25);x1=Math.max(x1,x+.25);z1=Math.max(z1,z+.25)}
      const y=-FLOOR,grow=.6;
      mesh.face([[x0-grow,y,z0-grow],[x0-grow,y,z1+grow],[x1+grow,y,z1+grow],[x1+grow,y,z0-grow]],[0,1,0],KIT.floor);
      if(map.cabinets)cabinets(mesh,map);
      for(const prop of bins){const from=mesh.data.length;bin(mesh,prop);paint(from,prop)}
    }
    // A Forts map: each base is a pad in its team's colour, the bacon lies on a plate, and the forcefield is a pane of blue glass (drawn last, see-through).
    const glass=new Mesh;
    for(const zone of map.zones??[]){
      if(zone.kind==='base'){const r=zone.rect,color=tint(rgb(TEAM_COLOR[zone.team]),zone);mesh.box(r[0],0,r[1],r[2],.004,r[3],color);
        for(const [x,z] of [[r[0],r[1]],[r[2],r[1]],[r[0],r[3]],[r[2],r[3]]])mesh.post(x,z,0,.09,.008,scale(color,.8))}
      else if(zone.kind==='field'){const r=zone.rect;glass.box(r[0],0,r[1],r[2],FIELD_HEIGHT,r[3],tint(rgb(FIELD),zone))}
      else{
        const [x,z]=zone.p,y=floorAt(map,x,z),lean=tint(rgb(BACON[0]),zone),fat=tint(rgb(BACON[1]),zone);
        mesh.post(x,z,y,y+.006,.075,tint(rgb('#fffaf0'),zone),tint(rgb('#fffaf0'),zone),20);
        for(const dz of [-.022,.022]){mesh.box(x-.055,y+.006,z+dz-.014,x+.055,y+.016,z+dz+.014,lean);mesh.box(x-.055,y+.016,z+dz-.004,x+.055,y+.018,z+dz+.004,fat)}
        mesh.post(x,z,y+.018,y+.2,.004,lean);mesh.gem(x,y+.22,z,.022,lean);
      }
    }
    for(const deck of map.decks){
      const r=deck.rect;
      if(deck.style!==ROUND){mesh.box(r[0],0,r[1],r[2],deck.height,r[3],tint(COLOR[deck.style],deck));continue}
      // A round deck is a bamboo steamer's lid: the oval inside its rect, a red rim round the top's edge.
      const at=[(r[0]+r[2])/2,0,(r[1]+r[3])/2],stretch=[(r[2]-r[0])/2,(r[3]-r[1])/2],h=deck.height;
      mesh.revolve(at,[[1,0],[1,h],[0,h]],tint(COLOR.bamboo,deck),{sides:40,stretch});
      mesh.revolve(at,[[.985,h+.0006],[.94,h+.0006]],tint(COLOR.red,deck),{sides:40,stretch});
    }
    for(const ramp of map.ramps){
      const from=mesh.data.length;
      // A ramp that names a style is a plank of it (a board propped in a sink, say); its foot may be below the counter.
      if(ramp.style!==undefined&&ramp.style!==null){
        mesh.plank(ramp.rect,ramp.base,ramp.height,ramp.rise,COLOR[ramp.style]??COLOR.wood);
        if(ramp.style==='wood')hangingHole(mesh,ramp);
      }else mesh.ramp(ramp.rect,ramp.base,ramp.height,ramp.rise);
      paint(from,ramp);
    }
    for(const block of map.blocks){
      const r=block.rect,kit=KITS.indexOf(block.style);
      if(kit>=FIRST_KITS){const from=mesh.data.length;thing(mesh,glass,block);paint(from,block);continue}
      // A slim bamboo block is a chopstick standing in a white holder.
      if(block.style===ROUND&&Math.max(r[2]-r[0],r[3]-r[1])<=.08){
        const x=(r[0]+r[2])/2,z=(r[1]+r[3])/2,radius=Math.min(r[2]-r[0],r[3]-r[1])/2;
        mesh.post(x,z,0,.03,radius,tint(KIT.ceramic,block));mesh.post(x,z,.03,block.height,.0045,tint(COLOR.bamboo,block));continue;
      }
      const color=tint(COLOR[block.style]??COLOR.steel,block);
      mesh.box(r[0],0,r[1],r[2],block.height,r[3],color,kit>=0?block.facing:null,kit>=0?scale(color,block.style==='microwave'?.45:.7):null);
    }
    for(const prop of map.props){
      if(prop.rect){const r=prop.rect;mesh.box(r[0],prop.base,r[1],r[2],prop.base+prop.height,r[3],tint(COLOR[prop.style],prop));continue}
      if(prop.kind==='bin')continue;
      if(prop.kind!=='turret'){
        // Loose objects are drawn from the same part lists the physics uses.
        const at=[prop.p[0],prop.base,prop.p[1]],color=tint(rgb(LOOSE_COLOR[prop.kind]??'#b9c1c4'),prop);
        for(const part of PRESETS[prop.kind]?.().parts??[]){
          if(part.type==='lathe')mesh.lathe([at[0]+part.center[0],at[1]+part.center[1],at[2]+part.center[2]],part.profile,color);
          else mesh.turned(at,prop.yaw,part.center,part.half,color);
        }
        continue;
      }
      const [x,z]=prop.p,[fx,fz]=facing(prop.yaw),y=prop.base,color=tint(GUN,prop);
      mesh.post(x,z,y,y+.05,.036,color,tint(YELLOW,prop));
      // The barrel: a thin box turned to face along the yaw.
      const sx=-fz*.009,sz=fx*.009,tip=[x+fx*.085,z+fz*.085],y0=y+.03,y1=y+.048;
      const corners=[[x+sx,z+sz],[tip[0]+sx,tip[1]+sz],[tip[0]-sx,tip[1]-sz],[x-sx,z-sz]];
      mesh.face(corners.map(c=>[c[0],y1,c[1]]),[0,1,0],scale(color,.6));
      for(let i=0;i<4;i++){const p=corners[i],q=corners[(i+1)%4];mesh.face([[p[0],y0,p[1]],[q[0],y0,q[1]],[q[0],y1,q[1]],[p[0],y1,p[1]]],[q[1]-p[1],0,p[0]-q[0]],scale(color,.6))}
    }
    // A gun floats as a gem in its colour; a box of ammunition is a small olive crate on the floor, with a darker lid and brass latches.
    for(const pickup of map.pickups){
      const [x,z]=pickup.p,y=floorAt(map,x,z),color=tint(rgb(weaponColor(pickup.weapon)),pickup);
      if(pickup.weapon!==AMMO_BOX){mesh.gem(x,y+.07,z,.028,color);continue}
      mesh.box(x-.03,y,z-.021,x+.03,y+.034,z+.021,color);
      mesh.box(x-.032,y+.034,z-.023,x+.032,y+.042,z+.023,scale(color,.72));
      for(const dx of [-.016,.016])mesh.box(x+dx-.004,y+.024,z+.021,x+dx+.004,y+.036,z+.0235,tint(BRASS,pickup));
    }
    // Zombies appear on the counter, or on a sink's floor (Level.spawnPosition).
    for(const spawn of map.spawns){const [x,z]=spawn.p,y=Math.min(floorAt(map,x,z),0);mesh.post(x,z,y,y+.11,.03,tint(ZOMBIE,spawn),tint(scale(ZOMBIE,.6),spawn))}
    map.starts.forEach((start,index)=>{
      // A hotdog standing on end: bun, sausage poking out of the top, a cap in the player's colour and a nose showing its facing.
      const [x,z]=start.p,y=floorAt(map,x,z),[fx,fz]=facing(start.yaw);
      mesh.post(x,z,y,y+.1,.034,tint(BUN,start));
      mesh.post(x,z,y+.1,y+.145,.024,tint(SAUSAGE,start),tint(rgb(TEAM_COLOR[start.team]??PLAYER[index%PLAYER.length]),start));
      mesh.box(x+fx*.04-.012,y+.05,z+fz*.04-.012,x+fx*.04+.012,y+.074,z+fz*.04+.012,tint(INK,start));
    });
    const gl=this.gl,lines=[],line=(x0,z0,x1,z1,shade)=>lines.push(x0,.0012,z0,0,1,0,shade,shade,shade,x1,.0012,z1,0,1,0,shade,shade,shade);
    // A rule along x or along z across the counter, broken where it crosses a sink.
    const rule=(x0,z0,x1,z1,shade)=>{
      const alongX=z0===z1,gaps=[];
      for(const {rect:r} of map.sinks??[])if(alongX?z0>r[1]&&z0<r[3]:x0>r[0]&&x0<r[2])gaps.push(alongX?[r[0],r[2]]:[r[1],r[3]]);
      gaps.sort((p,q)=>p[0]-q[0]);
      let from=alongX?x0:z0;
      const to=alongX?x1:z1,piece=(a,c)=>{if(c-a>1e-6)alongX?line(a,z0,c,z0,shade):line(x0,a,x0,c,shade)};
      for(const [a,c] of gaps){piece(from,Math.min(a,to));from=Math.max(from,c)}
      piece(from,to);
    };
    for(let i=Math.ceil(b[0]/.5);i*.5<=b[2];i++)rule(i*.5,b[1],i*.5,b[3],.62);
    for(let j=Math.ceil(b[1]/.5);j*.5<=b[3];j++)rule(b[0],j*.5,b[2],j*.5,.62);
    gl.bindBuffer(gl.ARRAY_BUFFER,this.solidBuffer);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array(mesh.data),gl.DYNAMIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER,this.lineBuffer);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array(lines),gl.DYNAMIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER,this.glassBuffer);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array(glass.data),gl.DYNAMIC_DRAW);
    this.count=mesh.data.length/9;this.lines=lines.length/9;this.glass=glass.data.length/9;
  }
  draw(){
    const gl=this.gl,{width,height}=this.canvas,cp=Math.cos(this.pitch);
    const eye=[this.target[0]+this.distance*cp*Math.sin(this.yaw),this.target[1]+this.distance*Math.sin(this.pitch),this.target[2]+this.distance*cp*Math.cos(this.yaw)];
    const matrix=multiply(perspective(FOV,width/height,Math.max(.01,this.distance*.02),this.distance*8+60),lookAt(eye,this.target));
    gl.viewport(0,0,width,height);gl.clearColor(.157,.196,.2,1);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);gl.disable(gl.CULL_FACE);gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.mvp,false,matrix);gl.uniform3fv(this.sun,SUN);
    // The glass goes last, blended over what stands behind it and leaving the depth as it was.
    for(const [buffer,mode,count,alpha] of [[this.solidBuffer,gl.TRIANGLES,this.count,1],[this.lineBuffer,gl.LINES,this.lines,1],[this.glassBuffer,gl.TRIANGLES,this.glass,GLASS]]){
      if(!count)continue;
      gl.uniform1f(this.alpha,alpha);
      if(alpha<1){gl.enable(gl.BLEND);gl.blendFunc(gl.SRC_ALPHA,gl.ONE_MINUS_SRC_ALPHA);gl.depthMask(false)}else{gl.disable(gl.BLEND);gl.depthMask(true)}
      gl.bindBuffer(gl.ARRAY_BUFFER,buffer);
      this.attributes.forEach((location,i)=>{gl.enableVertexAttribArray(location);gl.vertexAttribPointer(location,3,gl.FLOAT,false,36,i*12)});
      gl.drawArrays(mode,0,count);
    }
    gl.disable(gl.BLEND);gl.depthMask(true);
  }
  #controls(){
    const canvas=this.canvas;let held=null;
    canvas.addEventListener('pointerdown',event=>{
      try{canvas.setPointerCapture(event.pointerId)}catch{}
      held={x:event.clientX,y:event.clientY,slide:event.button!==0||event.shiftKey};
    });
    canvas.addEventListener('pointermove',event=>{
      if(!held)return;
      const dx=event.clientX-held.x,dy=event.clientY-held.y;
      held.x=event.clientX;held.y=event.clientY;
      if(held.slide){
        // Slide the point being looked at across the counter, in screen directions.
        const k=this.distance*.0016,cos=Math.cos(this.yaw),sin=Math.sin(this.yaw);
        this.target[0]-=(dx*cos+dy*sin)*k;this.target[2]-=(-dx*sin+dy*cos)*k;
      }else{this.yaw-=dx*.008;this.pitch=Math.min(1.52,Math.max(.1,this.pitch+dy*.006))}
      this.onchange?.();
    });
    const stop=()=>{held=null};
    canvas.addEventListener('pointerup',stop);canvas.addEventListener('pointercancel',stop);
    canvas.addEventListener('contextmenu',event=>event.preventDefault());
    canvas.addEventListener('wheel',event=>{
      event.preventDefault();
      const delta=event.deltaMode===1?event.deltaY*32:event.deltaY;
      this.distance=Math.min(200,Math.max(.15,this.distance*Math.exp(delta*.0015)));
      this.onchange?.();
    },{passive:false});
  }
}
