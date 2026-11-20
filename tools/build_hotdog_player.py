#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Export the original HD hotdog as glTF 2.0 with PBR maps and a walk animation."""
from pathlib import Path
import json
import math
import struct

ROOT=Path(__file__).resolve().parents[1]
TAU=math.tau
gltf={'asset':{'version':'2.0','generator':'Hotdog Simulator original procedural model','copyright':'Copyright (C) 2026 David Nunes-Childs; GPL-3.0-only'},'scene':0,'scenes':[{'nodes':[0]}],'nodes':[{'name':'HD Hotdog Player','children':[]}],'meshes':[],'materials':[],'accessors':[],'bufferViews':[],'buffers':[{}],'images':[],'textures':[],'samplers':[{'magFilter':9729,'minFilter':9987,'wrapS':10497,'wrapT':10497}]}
data=bytearray()

def accessor(values,kind='VEC3',component=5126):
    width={'SCALAR':1,'VEC2':2,'VEC3':3,'VEC4':4}[kind]
    while len(data)%4:data.append(0)
    offset=len(data);fmt='<'+('f' if component==5126 else 'I')*width
    for value in values:data.extend(struct.pack(fmt,*(value if width>1 else [value])))
    view=len(gltf['bufferViews']);gltf['bufferViews'].append({'buffer':0,'byteOffset':offset,'byteLength':len(data)-offset})
    item={'bufferView':view,'componentType':component,'count':len(values),'type':kind}
    if component==5126:
        rows=values if width>1 else [[v] for v in values]
        item['min']=[min(v[i] for v in rows) for i in range(width)];item['max']=[max(v[i] for v in rows) for i in range(width)]
    gltf['accessors'].append(item);return len(gltf['accessors'])-1

def texture(name):
    i=len(gltf['images']);gltf['images'].append({'uri':'../materials/food/'+name+'.png'});gltf['textures'].append({'sampler':0,'source':i});return i

def food(name):
    material={'name':name+' original 2K PBR','pbrMetallicRoughness':{'baseColorTexture':{'index':texture(name+'_albedo')},'metallicRoughnessTexture':{'index':texture(name+'_roughness')},'metallicFactor':0,'roughnessFactor':1},'normalTexture':{'index':texture(name+'_normal')}}
    gltf['materials'].append(material);return len(gltf['materials'])-1

def plain(name,color):
    gltf['materials'].append({'name':name,'pbrMetallicRoughness':{'baseColorFactor':color,'roughnessFactor':.45,'metallicFactor':0}});return len(gltf['materials'])-1

skin=food('sausage');bread=food('crust');cream=food('crumb')
mustard=plain('Mustard',[.97,.67,.04,1]);white=plain('Eye whites',[.97,.94,.84,1]);dark=plain('Eyes and mouth',[.025,.012,.009,1]);cyan=plain('BUNNER identity band',[.22,.79,.89,1])

def surface(name,rings,sides,sample,material):
    positions=[];normals=[];uv=[];indices=[]
    for i in range(rings+1):
        u=i/rings
        for j in range(sides+1):
            v=j/sides;p=sample(u,v);positions.append(p);uv.append([u,v])
            a=sample(min(1,u+.0001),v);b=sample(max(0,u-.0001),v);c=sample(u,v+.0001);d=sample(u,v-.0001)
            x=[a[k]-b[k] for k in range(3)];y=[c[k]-d[k] for k in range(3)];n=[x[1]*y[2]-x[2]*y[1],x[2]*y[0]-x[0]*y[2],x[0]*y[1]-x[1]*y[0]];length=math.sqrt(sum(q*q for q in n)) or 1;normals.append([q/length for q in n])
    for i in range(rings):
        for j in range(sides):
            a=i*(sides+1)+j;b=a+sides+1;indices.extend([a,b,a+1,a+1,b,b+1])
    mesh={'name':name,'primitives':[{'attributes':{'POSITION':accessor(positions),'NORMAL':accessor(normals),'TEXCOORD_0':accessor(uv,'VEC2')},'indices':accessor(indices,'SCALAR',5125),'material':material}]}
    gltf['meshes'].append(mesh);return len(gltf['meshes'])-1

def casing(u,v):
    t=abs(u*2-1);cap=math.sqrt(max(.0001,1-max(0,(t-.76)/.24)**2));a=v*TAU;r=.023*cap*(1+.025*math.sin(a*11+u*37)*t**8)
    return [.002*math.sin(u*math.pi)+r*math.cos(a),.023+u*.155,.002*math.sin(u*TAU)+r*math.sin(a)]

sausage=surface('Gathered sausage casing | 128 x 64',128,64,casing,skin)
bun=surface('Sculpted split bun | 80 x 40',80,40,lambda u,v:[.017*math.sqrt(max(.0001,math.sin(u*math.pi)))*math.cos(v*TAU),.031+u*.132,.026*math.sqrt(max(.0001,math.sin(u*math.pi)))*math.sin(v*TAU)+.012],bread)
sauce=surface('Raised mustard ribbon',96,12,lambda u,v:[math.sin(u*8*math.pi)*.010+math.cos(v*TAU)*.002,.047+u*.102,-.024+math.sin(v*TAU)*.002],mustard)
ball=surface('Smooth eye',24,24,lambda u,v:[math.sqrt(max(.00001,1-(2*u-1)**2))*math.cos(v*TAU),2*u-1,math.sqrt(max(.00001,1-(2*u-1)**2))*math.sin(v*TAU)],white)
limb=surface('Rounded limb',32,16,lambda u,v:[.004*math.sqrt(max(.0001,1-max(0,(abs(2*u-1)-.7)/.3)**2))*math.cos(v*TAU),(u-.5)*.032,.004*math.sqrt(max(.0001,1-max(0,(abs(2*u-1)-.7)/.3)**2))*math.sin(v*TAU)],skin)

def node(name,mesh,translation=None,scale=None,material=None):
    if material is not None:
        original=gltf['meshes'][mesh];copy={'name':name,'primitives':[dict(original['primitives'][0],material=material)]};gltf['meshes'].append(copy);mesh=len(gltf['meshes'])-1
    item={'name':name,'mesh':mesh,'translation':translation or [0,0,0],'rotation':[0,0,0,1],'scale':scale or [1,1,1]};gltf['nodes'].append(item);index=len(gltf['nodes'])-1;gltf['nodes'][0]['children'].append(index);return index

node('Sausage',sausage);node('Left bun',bun,[-.023,0,0]);node('Right bun',bun,[.023,0,0]);node('Mustard',sauce)
for side in [-1,1]:
    node('Eye white',ball,[side*.009,.153,-.022],[.007,.009,.0035])
    node('Pupil',ball,[side*.009,.153,-.0255],[.0035,.005,.002],dark)
    node('Eye sparkle',ball,[side*.009-.001,.155,-.027],[.0015]*3)
node('Mouth',ball,[0,.135,-.025],[.006,.003,.002],dark)
node('Identity badge',ball,[0,.072,-.028],[.012,.007,.002],cyan)
walk=[]
for side in [-1,1]:
    for leg in [True,False]:
        index=node(('Leg' if leg else 'Arm')+str(side),limb,[side*(.014 if leg else .052),.015 if leg else .08,0],material=cyan if leg else skin);walk.append((index,side))

times=accessor([i/16 for i in range(17)],'SCALAR');animation={'name':'Walk','samplers':[],'channels':[]}
for index,side in walk:
    rotations=[]
    for i in range(17):
        angle=math.sin(i/16*TAU+side*math.pi/2)*.45;rotations.append([math.sin(angle/2),0,0,math.cos(angle/2)])
    sampler=len(animation['samplers']);animation['samplers'].append({'input':times,'output':accessor(rotations,'VEC4'),'interpolation':'LINEAR'});animation['channels'].append({'sampler':sampler,'target':{'node':index,'path':'rotation'}})
gltf['animations']=[animation];gltf['buffers'][0]['byteLength']=len(data)
encoded=json.dumps(gltf,separators=(',',':')).encode();encoded+=b' '*((-len(encoded))%4);data+=b'\0'*((-len(data))%4)
out=ROOT/'assets/models/hotdog-player.glb';out.parent.mkdir(parents=True,exist_ok=True)
out.write_bytes(struct.pack('<4sII',b'glTF',2,12+8+len(encoded)+8+len(data))+struct.pack('<II',len(encoded),0x4e4f534a)+encoded+struct.pack('<II',len(data),0x004e4942)+data)
print(f'Exported {out.relative_to(ROOT)}: {out.stat().st_size:,} bytes; original geometry, 2K PBR maps, animated limbs.')
