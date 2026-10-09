import { useEffect, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Locale } from '../i18n';
type Disk = { path: string; enabled: boolean };
type Preferences = { ramPercent: number; historyStates: number; disks: Disk[] };
type Snapshot = { preferences: Preferences; totalRam: number; allocatedRam: number; processRss: number | null; scratchBytes: number; residentHistory: number; drives: { path: string; name: string; freeBytes: number; startup: boolean }[] };
export const memoryLabels = {
  ja: { performance:'パフォーマンス', scratch:'仮想記憶ディスク', ram:'メモリの使用状況', available:'搭載RAM', allocation:'LumaPaintに割り当てるRAMの目標', recommendation:'推奨範囲：55〜80%。初期設定は70%です。', process:'現在のプロセス使用量', historyMemory:'履歴メモリ（概算）', diskUsage:'仮想記憶ディスク使用量', history:'ヒストリー数', explanation:'割り当ては管理対象の履歴とキャッシュの予算です。作品本体、GPU、WebView、処理中の一時データを含むプロセス全体の上限ではありません。', description:'優先順位の高いディスクから履歴を退避します。空き容量が不足した場合は次の有効なディスクを使用します。少なくとも1GiBの空き容量を確保します。', enabled:'有効', drive:'ドライブ・保存先', free:'空き容量', startup:'起動ディスク', add:'保存先を追加…', up:'優先順位を上げる', down:'優先順位を下げる', save:'適用', cancel:'変更を取り消す', saved:'設定を適用しました', unavailable:'取得できません', loading:'読み込み中…', remove:'保存先を削除', units:'回', diskHint:'使用中の履歴は元のディスクに残し、新しい退避には変更後の設定を使用します。参照されなくなった退避ファイルは自動的に削除します。' },
  en: { performance:'Performance', scratch:'Scratch Disks', ram:'Memory usage', available:'Installed RAM', allocation:'RAM target for LumaPaint', recommendation:'Recommended range: 55–80%. Default: 70%.', process:'Current process memory', historyMemory:'History memory (estimate)', diskUsage:'Scratch disk usage', history:'History states', explanation:'Allocation budgets managed history and caches. It is not a hard process limit covering artwork, GPU, WebViews and temporary processing data.', description:'History is paged to disks in priority order. When space is insufficient, the next enabled disk is used. At least 1 GiB is reserved.', enabled:'Enabled', drive:'Drive / location', free:'Free space', startup:'Startup disk', add:'Add location…', up:'Increase priority', down:'Decrease priority', save:'Apply', cancel:'Revert changes', saved:'Settings applied', unavailable:'Unavailable', loading:'Loading…', remove:'Remove location', units:'states', diskHint:'Existing history stays on its original disk. New spills use the updated settings. Unreferenced scratch files are removed automatically.' },
  'zh-CN': { performance:'性能', scratch:'暂存盘', ram:'内存使用情况', available:'已安装的RAM', allocation:'LumaPaint的RAM使用目标', recommendation:'建议范围：55–80%。默认：70%。', process:'当前进程内存', historyMemory:'历史内存（估算）', diskUsage:'暂存盘使用量', history:'历史记录数', explanation:'分配目标用于管理历史和缓存，不是包含作品、GPU、WebView和临时处理数据的整个进程硬性上限。', description:'按优先顺序将历史存入磁盘。空间不足时使用下一个启用的磁盘。至少保留1GiB可用空间。', enabled:'启用', drive:'磁盘／保存位置', free:'可用空间', startup:'启动磁盘', add:'添加位置…', up:'提高优先级', down:'降低优先级', save:'应用', cancel:'撤销更改', saved:'设置已应用', unavailable:'无法获取', loading:'加载中…', remove:'删除位置', units:'条', diskHint:'现有历史保留在原磁盘，新写入使用更新的设置。不再被引用的暂存文件将自动删除。' },
};
const preview: Snapshot = { preferences:{ramPercent:70,historyStates:50,disks:[{path:'/',enabled:true}]}, totalRam:0,allocatedRam:0,processRss:null,scratchBytes:0,residentHistory:0,drives:[{path:'/',name:'System volume',freeBytes:0,startup:true}] };
export function MemorySettings({ locale, section }: { locale: Locale; section: 'performance' | 'scratch' }) {
  const t = memoryLabels[locale];
  const [snapshot,setSnapshot]=useState<Snapshot|null>(null);
  const [draft,setDraft]=useState<Preferences|null>(null);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState('');
  const [notice,setNotice]=useState('');
  const [selected,setSelected]=useState(0);
  useEffect(()=>{
    let live=true;
    const read=()=>isTauri()?invoke<Snapshot>('memory_settings'):Promise.resolve(preview);
    void read().then(next=>{if(live){setSnapshot(next);setDraft({...next.preferences,disks:[...next.preferences.disks,...next.drives.filter(d=>!next.preferences.disks.some(disk=>disk.path===d.path)).map(d=>({path:d.path,enabled:false}))]});}}).catch(e=>{if(live)setError(String(e));});
    const timer=setInterval(()=>{void read().then(next=>{if(live)setSnapshot(next);}).catch(e=>{if(live)setError(String(e));});},5000);
    return()=>{live=false;clearInterval(timer);};
  },[]);
  const format=(bytes:number)=>bytes?`${(bytes/1024**3).toLocaleString(locale,{maximumFractionDigits:2})} GiB`: '0 GiB';
  const change=(next:Preferences)=>{setDraft(next);setNotice('');};
  const move=(delta:number)=>{
    if(!draft)return;const index=selected+delta;if(index<0||index>=draft.disks.length)return;
    const disks=[...draft.disks];[disks[index],disks[selected]]=[disks[selected],disks[index]];change({...draft,disks});setSelected(index);
  };
  async function add(){
    setBusy(true);setError('');try{const path=isTauri()?await invoke<string|null>('memory_pick_disk'):null;
      if(path&&draft&&!draft.disks.some(d=>d.path===path)){change({...draft,disks:[...draft.disks,{path,enabled:true}]});setSelected(draft.disks.length);}
    }catch(e){setError(String(e));}finally{setBusy(false);}
  }
  async function save(){if(!draft)return;setBusy(true);setError('');setNotice('');try{
    const next=isTauri()?await invoke<Snapshot>('save_memory_settings',{preferences:draft}):{...preview,preferences:draft};setSnapshot(next);setDraft({...next.preferences,disks:[...next.preferences.disks,...next.drives.filter(d=>!next.preferences.disks.some(disk=>disk.path===d.path)).map(d=>({path:d.path,enabled:false}))]});setNotice(t.saved);
  }catch(e){setError(String(e));}finally{setBusy(false);}}
  if(!snapshot||!draft)return <div className="memory-settings"><p role={error?'alert':'status'}>{error||t.loading}</p></div>;
  return <div className="memory-settings">
    {section==='performance'?<>
      <h3>{t.ram}</h3>
      <dl><dt>{t.available}</dt><dd>{snapshot.totalRam?format(snapshot.totalRam):t.unavailable}</dd><dt>{t.process}</dt><dd>{snapshot.processRss===null?t.unavailable:format(snapshot.processRss)}</dd><dt>{t.historyMemory}</dt><dd>{format(snapshot.residentHistory)}</dd><dt>{t.diskUsage}</dt><dd>{format(snapshot.scratchBytes)}</dd></dl>
      <label className="memory-allocation">{t.allocation}<span><input aria-label={t.allocation} type="number" step="0.01" min={10} max={90} value={Number(draft.ramPercent.toFixed(2))} disabled={busy} onChange={e=>change({...draft,ramPercent:Number(e.target.value)})}/> % · <input type="number" aria-label={`${t.allocation} (MiB)`} disabled={busy||!snapshot.totalRam} min={Math.ceil(snapshot.totalRam/1024**2*.1)} max={Math.floor(snapshot.totalRam/1024**2*.9)} value={snapshot.totalRam?Math.round(snapshot.totalRam*draft.ramPercent/100/1024**2):0} onChange={e=>change({...draft,ramPercent:Number(e.target.value)*1024**2/snapshot.totalRam*100})}/> MiB</span></label>
      <input className="memory-slider" aria-label={`${t.allocation} (%)`} type="range" min={10} max={90} value={draft.ramPercent} disabled={busy} onChange={e=>change({...draft,ramPercent:Number(e.target.value)})}/>
      <p>{t.recommendation}</p><p>{t.explanation}</p>
      <label className="memory-history">{t.history}<input aria-label={t.history} type="number" min={1} max={1000} value={draft.historyStates} disabled={busy} onChange={e=>change({...draft,historyStates:Number(e.target.value)})}/>{t.units}</label>
    </>:<>
      <h3>{t.scratch}</h3><p>{t.description}</p>
      <div className="scratch-table-wrap"><table><thead><tr><th>#</th><th>{t.enabled}</th><th>{t.drive}</th><th>{t.free}</th></tr></thead><tbody>
        {draft.disks.map((disk,index)=>{const drive=snapshot.drives.find(d=>d.path===disk.path);return <tr key={disk.path} aria-selected={index===selected} onClick={()=>setSelected(index)}><td>{index+1}</td><td><input type="checkbox" aria-label={`${t.enabled}: ${disk.path}`} checked={disk.enabled} disabled={busy} onChange={e=>change({...draft,disks:draft.disks.map((d,i)=>i===index?{...d,enabled:e.target.checked}:d)})}/></td><td><button type="button" onClick={()=>setSelected(index)}>{drive?.startup?t.startup:drive?.name||disk.path}</button><small>{disk.path}</small></td><td>{drive?format(drive.freeBytes):t.unavailable}</td></tr>;})}
      </tbody></table></div>
      <div className="scratch-actions"><button type="button" onClick={()=>void add()} disabled={busy}>{t.add}</button><button type="button" onClick={()=>move(-1)} disabled={busy||selected===0} aria-label={t.up}>↑</button><button type="button" onClick={()=>move(1)} disabled={busy||selected>=draft.disks.length-1} aria-label={t.down}>↓</button><button type="button" disabled={busy||draft.disks.length<=1} onClick={()=>{change({...draft,disks:draft.disks.filter((_,index)=>index!==selected)});setSelected(0);}}>{t.remove}</button></div><p>{t.diskHint}</p>
    </>}
    {error&&<p role="alert" className="memory-error">{error}</p>}{notice&&<p role="status">{notice}</p>}
    <footer><button type="button" disabled={busy} onClick={()=>{change(snapshot.preferences);setError('');}}>{t.cancel}</button><button type="button" disabled={busy||draft.ramPercent<10||draft.ramPercent>90||draft.historyStates<1||draft.historyStates>1000||!draft.disks.some(d=>d.enabled)} onClick={()=>void save()}>{t.save}</button></footer>
  </div>;
}
