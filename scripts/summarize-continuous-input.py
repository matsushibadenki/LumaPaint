#!/usr/bin/env python3
"""Summarize accepted 16 x 1s native QA phases; this is not physical input latency.

The records JSON supplies tag/mode/kind/start/end/trialLog. Each non-idle
phase must contain all sixteen down + sixty moves + up timestamped sequences.
The first two gestures are warmup. Adjacent presentations are correlated.
"""
import argparse,json,re,math,hashlib,sys
parser=argparse.ArgumentParser(description=__doc__)
from pathlib import Path
parser.add_argument('records',type=Path)
parser.add_argument('process_samples',type=Path)
parser.add_argument('output',type=Path)
parser.add_argument('--binary',type=Path,required=True)
args=parser.parse_args()
ROOT=Path(__file__).resolve().parent.parent
records=json.loads(args.records.read_text());proc=[json.loads(l) for l in args.process_samples.read_text().splitlines()]
def dist(v):
 if not v:return None
 s=sorted(v);return {'samples':len(s),'p50_ms':s[math.ceil(len(s)*.5)-1]/1e6,'p95_ms':s[math.ceil(len(s)*.95)-1]/1e6,'max_ms':s[-1]/1e6}
SAMPLE=re.compile(r'^lumapaint-input-present first_event=(\d+) latest_event=(\d+) coalesced_events=(\d+) first_handler_to_present_host_ns=(\d+) latest_handler_to_present_host_ns=(\d+)')
QUEUE=re.compile(r'^lumapaint-input-queue event=(\d+) age_ns=(\d+) event_timestamp_ns=(\d+)')
STAGE=re.compile(r'^lumapaint-input-stage event=(\d+) stage=(pointer_down|pointer_drag|pointer_up|pan_down|pan_drag|pan_up) ')
STATE=re.compile(r'^lumapaint-pane-state document=(\d+) frame=(\d+) playing=true presented=true')
out={'date':'2026-10-25','transport':'timed producer -> Tauri main dispatch -> AppKit queue, synthetic native input; no direct model edits; not physical mouse/pen/input-to-photon','protocol':'same Release binary, old/new/new/old; 16 x 1sec/60Hz gestures per draw/pan phase, 150ms gaps; discard first two gestures; idle recovery 5sec','binary_sha256':hashlib.sha256(args.binary.read_bytes()).hexdigest(),'source_sha256':{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in ['src-tauri/src/pane_scheduling.rs','src-tauri/src/pane_gpu_macos.rs','src-tauri/src/canvas_macos.rs','src-tauri/src/input_timing_macos.rs','src-tauri/src/native_gesture_qa_macos.rs']},'trials':[]}
for r in records:
 lines=Path(r['trialLog']).read_text().splitlines();events={};kind={};source={};samples=[];frames={};advances=[];last={};policy={k:0 for k in ['Yield','Limited']};skips=0
 for i,line in enumerate(lines):
  if m:=QUEUE.match(line):events[int(m[1])]={'age_ns':int(m[2]),'timestamp_ns':int(m[3]),'line':i}
  if m:=STAGE.match(line):kind[int(m[1])]=m[2]
  if m:=SAMPLE.match(line):samples.append(tuple(map(int,m.groups())))
  if m:=re.match(r'lumapaint-qa-gesture tag=\S+ kind=\w+ index=(\d+) step=(0|61) timestamp_ns=(\d+)',line):source.setdefault(int(m[1]),{})[int(m[2])]=int(m[3])
  if m:=STATE.match(line):
   d,f=m[1],int(m[2]);frames.setdefault(d,[]).append(f)
   if last.get(d)!=f:advances.append((i,d,f))
   last[d]=f
  for k in policy:policy[k]+=line.startswith('lumapaint-pane-schedule budget='+k)
  skips+=line.startswith('lumapaint-pane-sync-skip ')
 def gesture(event):
  e=events.get(event)
  if not e:return None
  return next((g for g,b in source.items() if b.get(0,1e30)-10<=e['timestamp_ns']<=b.get(61,-1)+10),None)
 retained=[s for s in samples if (g:=gesture(s[1])) is not None and g>=2]
 spans=[];held_counts={};held_seconds=0
 for g,b in source.items():
  if 0 not in b or 61 not in b:continue
  down='pointer_down' if r['kind']=='draw' else 'pan_down';up='pointer_up' if r['kind']=='draw' else 'pan_up'
  downs=[e for id,e in events.items() if kind.get(id)==down and abs(e['timestamp_ns']-b[0])<=10];ups=[e for id,e in events.items() if kind.get(id)==up and abs(e['timestamp_ns']-b[61])<=10]
  spans.append({'index':g,'source_ms':(b[61]-b[0])/1e6,'down_received':len(downs),'up_received':len(ups),'moves':sum(kind.get(id)==('pointer_drag' if r['kind']=='draw' else 'pan_drag') and b[0]-10<e['timestamp_ns']<=b[61]+10 for id,e in events.items())})
  if g>=2 and downs and ups:
   held_seconds+=(b[61]-b[0])/1e9
   for i,d,f in advances:
    if downs[0]['line']<i<ups[-1]['line']:held_counts[d]=held_counts.get(d,0)+1
 if r['kind']!='idle' and not (len(spans)==16 and all(s['source_ms']==1000 and s['down_received']==1 and s['up_received']==1 and s['moves']==60 for s in spans)):
  raise ValueError(f"Incomplete timed input phase {r['planTag']}: {spans}")
 first=[s[3] for s in retained];latest=[s[4] for s in retained];paired=[s[4]+events[s[1]]['age_ns'] for s in retained if s[1] in events]
 ps=[p for p in proc if r['start']<=p['wall_ms']<=r['end']];wall=(r['end']-r['start'])/1000
 counts={d:sum(d2==d for _,d2,_ in advances) for d in frames}
 item={**r,'samples_total':len(samples),'retained_presentations':retained,'event_records':events,'event_kinds':kind,'gesture_spans':spans,'source_timestamps':source,'first_handler':dist(first),'latest_handler':dist(latest),'event_age':dist([events[s[1]]['age_ns'] for s in retained if s[1] in events]),'paired_event_to_present_upper_estimate':dist(paired),'policy_ticks':policy,'unchanged_pane_skips':skips,'background_presentations':{d:len(fs) for d,fs in frames.items()},'background_advances':counts,'background_advance_fps':{d:n/wall for d,n in counts.items()},'held_source_seconds':held_seconds,'held_advances':held_counts,'held_advance_fps':{d:n/held_seconds for d,n in held_counts.items()} if held_seconds else {},'process_samples':ps,'cpu_percent':((ps[-1]['cpu_seconds']-ps[0]['cpu_seconds'])/((ps[-1]['wall_ms']-ps[0]['wall_ms'])/1000)*100) if len(ps)>=2 else None,'rss_max_bytes':max((p['rss_bytes'] for p in ps),default=0)}
 out['trials'].append(item)
 print(r['planTag'],r['kind'],'n',len(retained),'latest',item['latest_handler'],'first',item['first_handler'],'heldfps',list(item['held_advance_fps'].values()),'skips',skips,'cpu',item['cpu_percent'])
out['pooled']={}
for k in ['draw','pan']:
 for mode in ['off','on']:
  ts=[t for t in out['trials'] if t['kind']==k and t['mode']==mode];ss=[s for t in ts for s in t['retained_presentations']];paired=[s[4]+t['event_records'][s[1]]['age_ns'] for t in ts for s in t['retained_presentations'] if s[1] in t['event_records']]
  out['pooled'][k+'-'+mode]={'gestures':14*len(ts),'first_handler':dist([s[3] for s in ss]),'latest_handler':dist([s[4] for s in ss]),'paired_upper_estimate':dist(paired)}
print('POOLED',out['pooled'])
for t in out['trials']:
 t['raw_log_sha256']=hashlib.sha256(Path(t['trialLog']).read_bytes()).hexdigest()
 t['retained_gestures']=[]
 for g in range(2,16):
  span=next((v for v in t['gesture_spans'] if v['index']==g),None)
  if span is None:continue
  # Match NSEvent timestamps with a 10ns tolerance for f64 conversion.
  bounds=t['source_timestamps'][g]
  ss=[s for s in t['retained_presentations'] if bounds[0]-10<=t['event_records'][s[1]]['timestamp_ns']<=bounds[61]+10]
  t['retained_gestures'].append({'index':g,'first_handler':dist([s[3] for s in ss]),'latest_handler':dist([s[4] for s in ss]),'coalesced_events':sum(s[2] for s in ss)})
 for k in ['retained_presentations','event_records','event_kinds','process_samples']:t.pop(k,None)
args.output.write_text(json.dumps(out,indent=2)+'\n')
