"""Original MIT craft model variants for the IonGram hexagon collection."""
import copy
import gzip
import json
from pathlib import Path
root=Path(__file__).resolve().parent.parent
base=json.loads(gzip.decompress((root/'assets/gifts/hexagon.tgs').read_bytes()))
for slug,title,palette in [
 ('crystal','Кристалл',[[.72,.94,1,1],[.15,.55,.9,1],[.03,.14,.4,1]]),
 ('plasma','Плазма',[[1,.25,.78,1],[.6,.09,.85,1],[.16,.02,.32,1]]),
 ('obsidian','Обсидиан',[[.22,.23,.29,1],[.08,.08,.12,1],[.02,.02,.035,1]])]:
 a=copy.deepcopy(base); a['nm']=title
 for n,l in enumerate(a['layers'][4:7]):
  for shape in l['shapes']:
   if shape['ty']=='fl':shape['c']['k']=palette[n]
   if shape['ty']=='st':shape['c']['k']=[.8,.95,1,1] if slug=='crystal' else ([1,.35,.86,1] if slug=='plasma' else [.7,.42,.95,1])
 inner=a['layers'][4]
 if slug=='crystal':
  inner['shapes'][0]['ks']['k']={'v':[[0,-100],[75,0],[0,100],[-75,0]],'i':[[0,0]]*4,'o':[[0,0]]*4,'c':True}
 elif slug=='plasma':
  inner['shapes']=[inner['shapes'][0],inner['shapes'][2]]
  inner['ks']['r']={'a':1,'k':[{'t':0,'s':[0],'e':[120],'i':{'x':[1],'y':[1]},'o':{'x':[0],'y':[0]}},{'t':120,'s':[120]}]}
  extra=copy.deepcopy(inner);extra['ind']=30;extra['ks']['s']={'a':0,'k':[55,55,100]};a['layers'].insert(0,extra)
 else:
  inner['shapes'][0]['ks']['k']={'v':[[-68,-60],[72,-12],[30,86]],'i':[[0,0]]*3,'o':[[0,0]]*3,'c':True}
  extra=copy.deepcopy(inner);extra['ind']=31;extra['ks']['s']={'a':0,'k':[-75,-75,100]};a['layers'].insert(0,extra)
 (root/f'assets/gifts/hexagon-{slug}.tgs').write_bytes(gzip.compress(json.dumps(a,separators=(',',':')).encode(),mtime=0))
