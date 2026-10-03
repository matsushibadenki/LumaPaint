import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import {useCallback,useEffect,useMemo,useRef,useState} from 'react';
import {imageFrameAction,imageLinks,manageImageLinks,placeImage,type DocumentSnapshot,type ImageLink,type FrameFit} from '../bridge';
import type {Locale} from '../i18n';
import './links-panel.css';

export const linkLabels={
 ja:{title:'リンク',place:'配置…',refresh:'リンクを確認',update:'更新',updateAll:'変更されたリンクをすべて更新',relink:'再リンク…',embed:'埋め込み',unembed:'埋め込みを解除…',empty:'空のフレーム',normal:'最新',modified:'変更あり',missing:'リンク切れ',embedded:'埋め込み',checking:'確認中',contain:'内容を縦横比を保持して合わせる',cover:'フレームに均等に流し込む',stretch:'内容をフレームに合わせる',fitting:'フレーム調整',offset:'内容を移動',apply:'移動',none:'画像フレームはありません。レイアウト用ツールで作成できます。',name:'名前',status:'状態',layer:'レイヤー',format:'形式',size:'サイズ',search:'リンクを検索',all:'すべて',issues:'問題のあるリンク',group:'同じリンクをまとめる',options:'パネルオプション',info:'リンク情報',path:'場所',dimensions:'画像寸法',modifiedAt:'元ファイルの更新日時',instances:'配置数',go:'配置箇所へ移動',selectAll:'表示中のリンクをすべて選択',clear:'選択解除',selected:'選択',noResults:'該当するリンクはありません。',multiRelink:'選択した配置を、同じ新しい画像に再リンクします。',selectionHint:'Shiftで範囲選択、⌘／Ctrlで追加選択。名前のダブルクリックで配置箇所を選択。'},
 en:{title:'Links',place:'Place…',refresh:'Check links',update:'Update',updateAll:'Update all modified links',relink:'Relink…',embed:'Embed',unembed:'Unembed…',empty:'Empty frame',normal:'Up to date',modified:'Modified',missing:'Missing',embedded:'Embedded',checking:'Checking',contain:'Fit content proportionally',cover:'Fill frame proportionally',stretch:'Fit content to frame',fitting:'Frame fitting',offset:'Move content',apply:'Move',none:'No image frames. Create one with the layout tools.',name:'Name',status:'Status',layer:'Layer',format:'Format',size:'Size',search:'Search links',all:'All',issues:'Problem links',group:'Group identical links',options:'Panel options',info:'Link information',path:'Location',dimensions:'Image dimensions',modifiedAt:'Source modified',instances:'Instances',go:'Go to placement',selectAll:'Select all visible links',clear:'Clear selection',selected:'selected',noResults:'No matching links.',multiRelink:'Relink the selected placements to the same new image.',selectionHint:'Shift selects a range; ⌘/Ctrl adds to selection. Double-click a name to select its placement.'},
 'zh-CN':{title:'链接',place:'置入…',refresh:'检查链接',update:'更新',updateAll:'更新所有已修改的链接',relink:'重新链接…',embed:'嵌入',unembed:'取消嵌入…',empty:'空框架',normal:'最新',modified:'已修改',missing:'链接丢失',embedded:'已嵌入',checking:'检查中',contain:'按比例适合内容',cover:'按比例填充框架',stretch:'使内容适合框架',fitting:'框架适配',offset:'移动内容',apply:'移动',none:'没有图像框架。请使用版面工具创建。',name:'名称',status:'状态',layer:'图层',format:'格式',size:'大小',search:'搜索链接',all:'全部',issues:'有问题的链接',group:'合并相同链接',options:'面板选项',info:'链接信息',path:'位置',dimensions:'图像尺寸',modifiedAt:'源文件修改时间',instances:'置入数量',go:'转到置入位置',selectAll:'选择所有可见链接',clear:'取消选择',selected:'已选择',noResults:'没有匹配的链接。',multiRelink:'将选中的置入对象重新链接到同一个新图像。',selectionHint:'Shift 选择范围；⌘/Ctrl 添加选择。双击名称选择置入对象。'},
};
type Status=ImageLink['status']|'checking';
type Sort='name'|'status'|'layer'|'format'|'size';
const symbols:Record<Status,string>={normal:'✓',modified:'▲',missing:'?',embedded:'▣',empty:'×',checking:'…'};
const priority:Record<Status,number>={missing:0,modified:1,checking:2,embedded:3,normal:4,empty:5};
const byteLabel=(n:number)=>n>=1048576?`${(n/1048576).toFixed(1)} MiB`:n>=1024?`${(n/1024).toFixed(1)} KiB`:`${n} B`;

const preferenceKey='lumapaint.links-panel.v1';
function readPreferences(){
 try{const value=JSON.parse(localStorage.getItem(preferenceKey)??'{}') as {group?:unknown;columns?:unknown;sort?:unknown;descending?:unknown};
 return {group:value.group!==false,columns:['name','status',...(['layer','format','size'] as const).filter(column=>Array.isArray(value.columns)?value.columns.includes(column):column==='layer')] as Sort[],sort:(['name','status','layer','format','size'].includes(String(value.sort))?value.sort:'name') as Sort,descending:value.descending===true};
 }catch{return {group:true,columns:['name','status','layer'] as Sort[],sort:'name' as Sort,descending:false};}
}

export function LinksPanel({locale,document,enabled,onUpdate}:{locale:Locale;document:DocumentSnapshot;enabled:boolean;onUpdate:(d:DocumentSnapshot)=>void}){
 const unit=useMeasurementUnit(),factor=pixelsPerMeasurement(unit,document.resolution),symbol=unitSymbols[unit];
 const t=linkLabels[locale];
 const [preferences]=useState(readPreferences);
 const [links,setLinks]=useState<ImageLink[]>([]),[busy,setBusy]=useState(false),[checking,setChecking]=useState(false),[error,setError]=useState('');
 const [selection,setSelection]=useState<string[]>(document.selectedVectorObjects),[anchor,setAnchor]=useState<string|null>(null);
 const [search,setSearch]=useState(''),[filter,setFilter]=useState('all'),[sort,setSort]=useState<Sort>(preferences.sort),[descending,setDescending]=useState(preferences.descending);
 const [group,setGroup]=useState(preferences.group),[expanded,setExpanded]=useState<string[]>([]),[columns,setColumns]=useState<Sort[]>(preferences.columns);
 const [offset,setOffset]=useState<[string,string]>(['0','0']);
 useEffect(()=>setOffset(['0','0']),[unit]);
 const live=useRef(true),request=useRef(0);
 useEffect(()=>{live.current=true;return()=>{live.current=false;request.current++;};},[]);
 useEffect(()=>{try{localStorage.setItem(preferenceKey,JSON.stringify({group,columns,sort,descending}));}catch{/* Preferences are optional. */}},[group,columns,sort,descending]);
 const frames=useMemo(()=>document.layers.flatMap(layer=>layer.objects.filter(object=>object.imageFrame).map(object=>({layer,object}))),[document.layers]);
 const signature=JSON.stringify(frames.map(f=>[f.object.id,f.object.imageFrame]));
 const canvasSelection=JSON.stringify(document.selectedVectorObjects);
 useEffect(()=>{setSelection(JSON.parse(canvasSelection) as string[]);setAnchor(null);},[canvasSelection]);
 const refresh=useCallback(async()=>{
   const generation=++request.current;setChecking(true);
   try{const result=await imageLinks();if(live.current&&generation===request.current){setLinks(result);setError('');}}
   catch(e){if(live.current&&generation===request.current)setError(String(e));}
   finally{if(live.current&&generation===request.current)setChecking(false);}
 },[]);
 useEffect(()=>{void refresh();},[signature,refresh]);
 useEffect(()=>{const focus=()=>void refresh();window.addEventListener('focus',focus);return()=>window.removeEventListener('focus',focus);},[refresh]);
 const linkMap=useMemo(()=>new Map(links.map(link=>[link.id,link])),[links]);
 const rows=useMemo(()=>frames.map(f=>({...f,link:linkMap.get(f.object.id),status:(linkMap.get(f.object.id)?.status??(f.object.imageFrame?.name?'checking':'empty')) as Status,name:f.object.imageFrame?.name??f.object.name})),[frames,linkMap]);
 const selected=rows.filter(r=>selection.includes(r.object.id));
 const editable=(r:typeof rows[number])=>enabled&&!busy&&r.layer.visible&&!r.layer.locked&&r.object.visible&&!r.object.locked;
 const canManage=selected.length>0&&selected.every(r=>editable(r)&&r.object.imageFrame?.name);
 const modified=rows.filter(r=>r.status==='modified'&&editable(r));
 const visible=useMemo(()=>rows.filter(r=>(filter==='all'||filter==='issues'?(filter==='all'||r.status==='modified'||r.status==='missing'):r.status===filter)&&[r.name,r.layer.name,r.object.imageFrame?.sourcePath??''].join(' ').toLocaleLowerCase(locale).includes(search.toLocaleLowerCase(locale))).sort((a,b)=>{
   let comparison=sort==='status'?priority[a.status]-priority[b.status]:sort==='size'?(a.link?.bytes??0)-(b.link?.bytes??0):String(sort==='name'?a.name:sort==='layer'?a.layer.name:a.link?.format??'').localeCompare(String(sort==='name'?b.name:sort==='layer'?b.layer.name:b.link?.format??''),locale,{numeric:true});
   if(!comparison)comparison=a.object.id.localeCompare(b.object.id);return descending?-comparison:comparison;
 }),[rows,filter,search,sort,descending,locale]);
 const display=useMemo(()=>{
   const groups=new Map<string,typeof rows>();
 for(const row of visible){const key=group&&row.object.imageFrame?.sourcePath?`source:${row.object.imageFrame.sourcePath}`:row.object.id;const list=groups.get(key)??[];list.push(row);groups.set(key,list);}
   const result:Array<{key:string;row:typeof rows[number];ids:string[];children?:typeof rows;child?:boolean}>=[];
 for(const [key,items] of groups){result.push({key,row:items[0],ids:items.map(r=>r.object.id),children:items.length>1?items:undefined});if(items.length>1&&expanded.includes(key))for(const row of items)result.push({key:row.object.id,row,ids:[row.object.id],child:true});}
   return result;
 },[visible,group,expanded]);
 function choose(ids:string[],key:string,extend:boolean,range:boolean){
   const index=display.findIndex(r=>r.key===key),start=display.findIndex(r=>r.key===anchor);
   if(range&&start>=0){setSelection([...new Set(display.slice(Math.min(start,index),Math.max(start,index)+1).flatMap(r=>r.ids))]);}
   else if(extend)setSelection(old=>ids.every(id=>old.includes(id))?old.filter(id=>!ids.includes(id)):[...new Set([...old,...ids])]);
   else setSelection(ids);
   if(!range)setAnchor(key);
 }
 async function run(action:'update'|'relink'|'embed',ids=selected.map(r=>r.object.id)){
   if(!ids.length)return;setBusy(true);setError('');
   try{const next=await manageImageLinks(ids,action);if(live.current){onUpdate(next);await refresh();}}
   catch(e){if(live.current)setError(String(e));}finally{if(live.current)setBusy(false);}
 }
 async function single(action:'place'|FrameFit|'offset'|'go'){
   const row=selected[0];if(!row)return;setBusy(true);setError('');
   try{const next=action==='place'?await placeImage(row.object.id):await imageFrameAction(row.object.id,action,action==='offset'?offset.map(n=>Number(n)*factor):undefined);if(live.current){onUpdate(next);await refresh();}}
   catch(e){if(live.current)setError(String(e));}finally{if(live.current)setBusy(false);}
 }
 async function go(id:string){setBusy(true);try{const next=await imageFrameAction(id,'go');if(live.current)onUpdate(next);}catch(e){if(live.current)setError(String(e));}finally{if(live.current)setBusy(false);}}
 const singleRow=selected.length===1?selected[0]:undefined;
 const count=rows.filter(r=>r.object.imageFrame?.name).length;
 return <div className="links-panel" aria-busy={busy}>
  <header><h2>{t.title} <span>{count}</span></h2><button disabled={busy||checking} onClick={()=>void refresh()}>{checking?t.checking:t.refresh}</button></header>
  <div className="link-filters"><input type="search" aria-label={t.search} placeholder={t.search} value={search} onChange={e=>setSearch(e.target.value)}/><select aria-label={t.status} value={filter} onChange={e=>setFilter(e.target.value)}>{(['all','issues','missing','modified','embedded','normal','empty'] as const).map(value=><option key={value} value={value}>{t[value]}</option>)}</select></div>
  <details className="link-options"><summary>{t.options}</summary><label><input type="checkbox" checked={group} onChange={e=>setGroup(e.target.checked)}/>{t.group}</label><div>{(['layer','format','size'] as const).map(column=><label key={column}><input type="checkbox" checked={columns.includes(column)} onChange={e=>setColumns(old=>e.target.checked?[...old,column]:old.filter(c=>c!==column))}/>{t[column]}</label>)}</div></details>
  <div className="link-table-scroll" onKeyDown={e=>{if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='a'){e.preventDefault();e.stopPropagation();setSelection(visible.map(r=>r.object.id));}else if(e.key==='Escape'){e.preventDefault();e.stopPropagation();setSelection([]);}}}><table className="link-table" aria-label={t.title}><thead><tr>{columns.map(column=><th key={column} scope="col" aria-sort={sort===column?(descending?'descending':'ascending'):'none'}><button onClick={()=>{if(sort===column)setDescending(v=>!v);else{setSort(column);setDescending(false);}}}>{t[column]}{sort===column?(descending?' ↓':' ↑'):''}</button></th>)}</tr></thead><tbody>
  {display.map(({key,row,ids,children,child})=>{
    const chosen=ids.some(id=>selection.includes(id)),allSelected=ids.every(id=>selection.includes(id));
    const worst=children?[...children].sort((a,b)=>priority[a.status]-priority[b.status])[0].status:row.status;
    return <tr key={key} className={`${chosen?'selected ':''}${child?'instance':''}`} aria-selected={allSelected} onClick={e=>choose(ids,key,e.metaKey||e.ctrlKey,e.shiftKey)}>
     {columns.map(column=><td key={column}>{column==='name'?<div className="link-row-name">{children&&<button className="link-disclosure" aria-label={row.name} aria-expanded={expanded.includes(key)} onClick={e=>{e.stopPropagation();setExpanded(old=>old.includes(key)?old.filter(k=>k!==key):[...old,key]);}}>{expanded.includes(key)?'▾':'▸'}</button>}<button className="link-name" title={row.object.imageFrame?.sourcePath??row.name} onClick={e=>{e.stopPropagation();choose(ids,key,e.metaKey||e.ctrlKey,e.shiftKey);}} onDoubleClick={()=>{if(editable(row))void go(row.object.id);}}>{child?row.object.name:row.name}{children&&` (${children.length})`}</button></div>:column==='status'?<span className={`link-status ${worst}`} title={t[worst]} aria-label={t[worst]}>{symbols[worst]}</span>:column==='layer'?<span title={row.layer.name}>{children?new Set(children.map(r=>r.layer.name)).size===1?row.layer.name:'—':row.layer.name}</span>:column==='format'?row.link?.format??'—':row.link?byteLabel(row.link.bytes):'—'}</td>)}
    </tr>;
  })}
  </tbody></table></div>
  {!display.length&&<p>{frames.length?t.noResults:t.none}</p>}
  <div className="link-selection-bar"><span>{selected.length} {t.selected}</span><button disabled={!visible.length||busy} onClick={()=>setSelection(visible.map(r=>r.object.id))}>{t.selectAll}</button><button disabled={!selected.length||busy} onClick={()=>setSelection([])}>{t.clear}</button></div>
  <div className="link-actions" aria-label={t.title}>
   <button disabled={!canManage||selected.some(r=>!r.object.imageFrame?.sourcePath||r.status==='missing')} onClick={()=>void run('update')}>{t.update}</button>
   <button disabled={!canManage} title={selected.length>1?t.multiRelink:undefined} onClick={()=>void run('relink')}>{selected.length&&selected.every(r=>r.status==='embedded')?t.unembed:t.relink}</button>
   <button disabled={!canManage||selected.some(r=>!r.object.imageFrame?.sourcePath)} onClick={()=>void run('embed')}>{t.embed}</button>
   <button disabled={!singleRow||!editable(singleRow)} onClick={()=>void single('go')}>{t.go}</button>
   {singleRow?.status==='empty'&&<button disabled={!editable(singleRow)} onClick={()=>void single('place')}>{t.place}</button>}
   <button disabled={!modified.length||checking} onClick={()=>void run('update',modified.map(r=>r.object.id))}>{t.updateAll} ({modified.length})</button>
  </div>
  <p className="link-hint">{t.selectionHint}</p>
  {singleRow&&<details className="link-info" open><summary>{t.info}</summary><dl>
   <dt>{t.name}</dt><dd>{singleRow.name}</dd><dt>{t.status}</dt><dd>{t[singleRow.status]}</dd>
   <dt>{t.path}</dt><dd>{singleRow.object.imageFrame?.sourcePath??'—'}</dd><dt>{t.layer}</dt><dd>{singleRow.layer.name} · {singleRow.object.name}</dd>
   <dt>{t.format}</dt><dd>{singleRow.link?.format||'—'}</dd><dt>{t.size}</dt><dd>{singleRow.link?byteLabel(singleRow.link.bytes):'—'}</dd>
   <dt>{t.dimensions}</dt><dd>{singleRow.object.imageFrame?.size?.join(' × ')??'—'} px</dd>
   <dt>{t.instances}</dt><dd>{singleRow.object.imageFrame?.sourcePath?rows.filter(r=>r.object.imageFrame?.sourcePath===singleRow.object.imageFrame?.sourcePath).length:1}</dd>
   <dt>{t.modifiedAt}</dt><dd>{singleRow.link?.modifiedAt?new Date(singleRow.link.modifiedAt*1000).toLocaleString(locale):'—'}</dd>
  </dl></details>}
  {singleRow?.object.imageFrame?.name&&<fieldset disabled={!editable(singleRow)}><legend>{t.fitting}</legend>{(['contain','cover','stretch'] as const).map(fit=><button key={fit} aria-pressed={singleRow.object.imageFrame?.fitting===fit} onClick={()=>void single(fit)}>{t[fit]}</button>)}
   <form onSubmit={e=>{e.preventDefault();void single('offset');}}><span>{t.offset}</span>{(['X','Y'] as const).map((axis,i)=><label key={axis}>{axis} ({symbol})<input type="number" min={-100000/factor} max={100000/factor} value={offset[i]} onChange={e=>setOffset(v=>v.map((x,j)=>j===i?e.target.value:x) as [string,string])}/></label>)}<button disabled={offset.some(v=>v===''||!Number.isFinite(Number(v)))}>{t.apply}</button></form>
  </fieldset>}
  {error&&<p role="alert">{error}</p>}
 </div>;
}
