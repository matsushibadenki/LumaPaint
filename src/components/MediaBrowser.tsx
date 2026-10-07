import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { memo, useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import type { Locale } from '../i18n';
import { Icon } from './Icon';
import './media-browser.css';

export const mediaBrowserTitle = { ja: '画像・動画ブラウザー', en: 'Media browser', 'zh-CN': '图片与视频浏览器' };
const labels = {
  ja: { folder:'フォルダーを開く', up:'上のフォルダー', back:'戻る', refresh:'再読み込み', close:'閉じる', search:'ファイル名で検索', places:'よく使う場所', folders:'フォルダー', content:'コンテンツ', preview:'プレビュー', info:'ファイル情報', all:'すべて', image:'画像', video:'動画', name:'名前順', date:'更新日時順', size:'サイズ順', empty:'画像・動画が見つかりません', choose:'フォルダーを選んで素材を探す', select:'プレビューするファイルを選択', loading:'読み込み中…', unavailable:'プレビューを生成できません', codec:'この動画形式は再生できません。別のコーデックで保存された動画をお試しください。', type:'形式', bytes:'ファイルサイズ', modified:'更新日時', dimensions:'プレビューサイズ', count:'件', more:'さらに読み込む', path:'フォルダーパス', go:'移動', thumb:'サムネイルのサイズ', limit:'表示件数の上限に達しました。サブフォルダーを指定して絞り込んでください。', ready:'サムネイルはキャッシュから再利用されます', play:'動画を再生', proxy:'軽量プレビュー', retry:'再試行' },
  en: { folder:'Open folder', up:'Parent folder', back:'Back', refresh:'Refresh', close:'Close', search:'Search filenames', places:'Places', folders:'Folders', content:'Content', preview:'Preview', info:'File information', all:'All media', image:'Images', video:'Videos', name:'Name', date:'Modified', size:'Size', empty:'No images or videos found', choose:'Choose a folder to browse your media', select:'Select a file to preview', loading:'Loading…', unavailable:'Preview unavailable', codec:'This video codec cannot be played. Try a video saved with another codec.', type:'Format', bytes:'File size', modified:'Modified', dimensions:'Preview dimensions', count:'items', more:'Load more', path:'Folder path', go:'Go', thumb:'Thumbnail size', limit:'The folder listing limit was reached. Choose a subfolder to narrow the list.', ready:'Thumbnails are reused from the cache', play:'Play video', proxy:'Lightweight preview', retry:'Retry' },
  'zh-CN': { folder:'打开文件夹', up:'上级文件夹', back:'返回', refresh:'刷新', close:'关闭', search:'搜索文件名', places:'常用位置', folders:'文件夹', content:'内容', preview:'预览', info:'文件信息', all:'全部', image:'图片', video:'视频', name:'名称', date:'修改时间', size:'大小', empty:'未找到图片或视频', choose:'选择文件夹以浏览素材', select:'选择文件以预览', loading:'正在加载…', unavailable:'无法生成预览', codec:'无法播放此视频编码，请尝试使用其他编码保存的视频。', type:'格式', bytes:'文件大小', modified:'修改时间', dimensions:'预览尺寸', count:'项', more:'加载更多', path:'文件夹路径', go:'前往', thumb:'缩略图大小', limit:'已达到显示数量上限，请指定子文件夹以缩小范围。', ready:'重复使用缓存中的缩略图', play:'播放视频', proxy:'轻量预览', retry:'重试' },
};
type Entry = { id:number; name:string; kind:'image'|'video'; extension:string; bytes:number; modified:number };
type Folder = { name:string; path:string };
type Scan = { id:number; path:string; parent:string|null; places:Folder[]; folders:Folder[]; truncated:boolean };
type Page = { entries:Entry[]; total:number };
const url = (session:number, id:number, size:'thumb'|'preview'|'video') => `${convertFileSrc(String(session), 'media')}/${id}/${size}`;
const fileSize = (bytes:number) => bytes < 1024 ? `${bytes} B` : bytes < 1048576 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / 1048576).toFixed(1)} MB`;
const Thumbnail = memo(function Thumbnail({ session, item, selected, onSelect, unavailable }:{session:number;item:Entry;selected:boolean;onSelect:(item:Entry)=>void;unavailable:string}) {
  const [failed,setFailed]=useState(false);
  return <button className={`media-card${selected?' selected':''}`} role="option" aria-selected={selected} onClick={()=>onSelect(item)} title={item.name}>
    <span className="media-card-image">{failed?<span className="media-fallback"><Icon name={item.kind==='video'?'animation':'image'}/><small>{unavailable}</small></span>:<img src={url(session,item.id,'thumb')} alt="" decoding="async" onError={()=>setFailed(true)}/>}{item.kind==='video'&&<span className="media-video-badge"><Icon name="animation"/>{item.extension.toUpperCase()}</span>}</span>
    <span className="media-card-name">{item.name}</span><span className="media-card-meta">{item.extension.toUpperCase()} <span>{fileSize(item.bytes)}</span></span>
  </button>;
});
function Preview({session,item,t,locale}:{session:number;item:Entry;t:typeof labels.en;locale:Locale}) {
  const [failed,setFailed]=useState(false), [playing,setPlaying]=useState(false), [dimensions,setDimensions]=useState(''), [loaded,setLoaded]=useState(false);
  return <>
    <div className="media-preview-image">{!playing&&!failed&&!loaded&&<span className="media-preview-loading" role="status">{t.loading}</span>}{playing?<video controls autoPlay playsInline src={url(session,item.id,'video')} onError={()=>setFailed(true)}/>:failed?<div className="media-empty"><Icon name="image"/><p>{t.unavailable}</p></div>:<img src={url(session,item.id,'preview')} alt={item.name} onLoad={e=>{setLoaded(true);setDimensions(`${e.currentTarget.naturalWidth} × ${e.currentTarget.naturalHeight}`);}} onError={()=>setFailed(true)}/>}
      {item.kind==='video'&&!playing&&<button className="media-play" onClick={()=>{setFailed(false);setPlaying(true);}}><Icon name="animation"/>{t.play}</button>}
    </div>
    {playing&&failed&&<p className="media-warning" role="status">{t.codec}</p>}
    <div className="media-preview-caption"><strong>{item.name}</strong><small>{item.kind==='video'?t.video:t.proxy}</small></div>
    <h3>{t.info}</h3><dl className="media-details"><dt>{t.type}</dt><dd>{item.extension.toUpperCase()}</dd><dt>{t.bytes}</dt><dd>{fileSize(item.bytes)}</dd><dt>{t.modified}</dt><dd>{new Date(item.modified*1000).toLocaleString(locale)}</dd>{dimensions&&<><dt>{t.dimensions}</dt><dd>{dimensions}</dd></>}</dl>
  </>;
}
export function MediaBrowser({locale,onClose}:{locale:Locale;onClose:()=>void}) {
  const t=labels[locale];
  const [top,setTop]=useState(30), [scan,setScan]=useState<Scan|null>(null), [items,setItems]=useState<Entry[]>([]), [total,setTotal]=useState(0);
  const [query,setQuery]=useState(''), [filter,setFilter]=useState('all'), [sort,setSort]=useState('name'), [path,setPath]=useState('');
  const [selected,setSelected]=useState<Entry|null>(null), [loading,setLoading]=useState(false), [error,setError]=useState(''), [history,setHistory]=useState<string[]>([]);
  const [thumb,setThumb]=useState(170), [viewport,setViewport]=useState({width:600,height:600,scroll:0});
  const grid=useRef<HTMLDivElement>(null), root=useRef<HTMLElement>(null), generation=useRef(0), paging=useRef(false), scanRef=useRef<Scan|null>(null);
  const pageGeneration=useRef(0);
  const navigate=useCallback(async (target?:string,record=true)=>{
    const current=++generation.current;if(scanRef.current)void invoke('media_interest',{session:scanRef.current.id,ids:[],preview:null}).catch(()=>{});pageGeneration.current++;paging.current=false;setLoading(true);setError('');setSelected(null);setItems([]);setTotal(0);setScan(null);
    try {const result=await invoke<Scan>('media_scan',{path:target??null});if(current!==generation.current)return;
      const previousPath=scanRef.current?.path;
      if(record&&previousPath&&previousPath!==result.path)setHistory(h=>[...h,previousPath].slice(-30));
      scanRef.current=result;setScan(result);setPath(result.path);
    } catch(e) {if(current===generation.current)setError(String(e));} finally {if(current===generation.current)setLoading(false);}
  },[]);
  useEffect(()=>{void navigate();return()=>{generation.current++;pageGeneration.current++;void invoke('media_cancel_scan').catch(()=>{});if(scanRef.current)void invoke('media_interest',{session:scanRef.current.id,ids:[],preview:null}).catch(()=>{});};},[navigate]);
  useEffect(()=>{const header=document.querySelector('.application-bar');const update=()=>setTop(header?.getBoundingClientRect().bottom??0);const observer=new ResizeObserver(update);if(header)observer.observe(header);update();return()=>observer.disconnect();},[]);
  useEffect(()=>{const el=grid.current;if(!el)return;const observer=new ResizeObserver(()=>setViewport(v=>({...v,width:el.clientWidth,height:el.clientHeight})));observer.observe(el);return()=>observer.disconnect();},[]);
  useEffect(()=>{const previous=document.activeElement as HTMLElement|null;root.current?.focus();const key=(e:KeyboardEvent)=>{if(e.key==='Escape'){e.preventDefault();onClose();}};window.addEventListener('keydown',key);return()=>{window.removeEventListener('keydown',key);previous?.focus();};},[onClose]);
  useEffect(()=>{
    const current=++pageGeneration.current;paging.current=false;setItems([]);setSelected(null);setTotal(0);if(!scan)return;
    setLoading(true);setError('');if(grid.current)grid.current.scrollTop=0;
    const timer=setTimeout(()=>{void invoke<Page>('media_page',{session:scan.id,query,filter,sort,offset:0}).then(result=>{if(current!==pageGeneration.current)return;setItems(result.entries);setTotal(result.total);}).catch(e=>{if(current===pageGeneration.current)setError(String(e));}).finally(()=>{if(current===pageGeneration.current)setLoading(false);});},150);
    return()=>clearTimeout(timer);
  },[scan,query,filter,sort]);
  const more=useCallback(async()=>{if(!scan||paging.current||loading||items.length>=total)return;const current=pageGeneration.current;paging.current=true;
    try {const result=await invoke<Page>('media_page',{session:scan.id,query,filter,sort,offset:items.length});if(current===pageGeneration.current)setItems(previous=>[...previous,...result.entries]);}catch(e){if(current===pageGeneration.current)setError(String(e));}finally{if(current===pageGeneration.current)paging.current=false;}
  },[scan,loading,items.length,total,query,filter,sort]);
  const columns=Math.max(1,Math.floor((viewport.width-24)/(thumb+12))), rowHeight=thumb+64;
  const start=Math.max(0,Math.floor(viewport.scroll/rowHeight)-1)*columns;
  const end=Math.min(items.length,(Math.ceil((viewport.scroll+viewport.height)/rowHeight)+1)*columns);
  useEffect(()=>{if(end>=items.length-columns*2&&items.length>0)void more();},[end,items.length,columns,more]);
  useEffect(()=>{if(!scan)return;const timer=setTimeout(()=>{void invoke('media_interest',{session:scan.id,ids:items.slice(start,end).map(item=>item.id),preview:selected?.id??null}).catch(()=>{});},80);return()=>clearTimeout(timer);},[scan,items,start,end,selected?.id]);
  const pick=async()=>{try{const target=await invoke<string|null>('media_pick_folder');if(target)void navigate(target);}catch(e){setError(String(e));}};
  const close=()=>onClose();
  const navigateCards=(event:ReactKeyboardEvent<HTMLDivElement>)=>{
    const delta=({ArrowRight:1,ArrowLeft:-1,ArrowDown:columns,ArrowUp:-columns} as Record<string,number>)[event.key];
    if(delta===undefined||!items.length)return;event.preventDefault();
    const index=items.findIndex(item=>item.id===selected?.id);const next=Math.max(0,Math.min(items.length-1,index<0?0:index+delta));setSelected(items[next]);
    if(grid.current){const y=Math.floor(next/columns)*rowHeight;if(y<grid.current.scrollTop)grid.current.scrollTop=y;else if(y+rowHeight>grid.current.scrollTop+grid.current.clientHeight)grid.current.scrollTop=y+rowHeight-grid.current.clientHeight;}
  };
  return <section ref={root} tabIndex={-1} className="media-browser" id="media-browser-overlay" role="dialog" aria-label={mediaBrowserTitle[locale]} style={{top}}>
    <header className="media-toolbar"><div className="media-heading"><Icon name="image"/><h2>{mediaBrowserTitle[locale]}</h2></div><button onClick={()=>void pick()}><Icon name="open"/>{t.folder}</button><span className="media-toolbar-space"/><button className="media-close" aria-label={t.close} title={t.close} onClick={close}>×</button></header>
    <div className="media-location"><button disabled={!history.length||loading} title={t.back} aria-label={t.back} onClick={()=>{const next=history.at(-1);setHistory(h=>h.slice(0,-1));void navigate(next,false);}}>←</button><button disabled={!scan?.parent||loading} title={t.up} aria-label={t.up} onClick={()=>void navigate(scan?.parent??undefined)}>↑</button><button disabled={loading} title={t.refresh} aria-label={t.refresh} onClick={()=>void navigate(scan?.path,false)}>↻</button><form onSubmit={e=>{e.preventDefault();if(path.trim())void navigate(path.trim());}}><Icon name="open"/><input aria-label={t.path} value={path} onChange={e=>setPath(e.target.value)} placeholder={t.path}/><button type="submit">{t.go}</button></form><input className="media-search" type="search" value={query} onChange={e=>setQuery(e.target.value)} placeholder={t.search} aria-label={t.search}/></div>
    {error&&<div className="media-warning" role="alert">{error}<button onClick={()=>void navigate(path||undefined,false)}>{t.retry}</button></div>}
    <div className="media-body"><aside className="media-sidebar"><h3>{t.places}</h3><nav aria-label={t.places}>{scan?.places.map(f=><button key={f.path} title={f.path} className={scan.path===f.path?'active':''} onClick={()=>void navigate(f.path)}><Icon name="open"/><span>{f.name}</span></button>)}</nav><h3>{t.folders}</h3><nav aria-label={t.folders}>{scan?.folders.map(f=><button key={f.path} title={f.path} onClick={()=>void navigate(f.path)}><Icon name="open"/><span>{f.name}</span><span>›</span></button>)}</nav></aside>
    <section className="media-content"><div className="media-content-toolbar"><h3>{t.content}<span>{total.toLocaleString()} {t.count}</span></h3><select aria-label={t.type} value={filter} onChange={e=>setFilter(e.target.value)}><option value="all">{t.all}</option><option value="image">{t.image}</option><option value="video">{t.video}</option></select><select aria-label={t.name} value={sort} onChange={e=>setSort(e.target.value)}><option value="name">{t.name}</option><option value="date">{t.date}</option><option value="size">{t.size}</option></select></div>
      <div ref={grid} className="media-grid-scroll" onKeyDown={navigateCards} onScroll={e=>{const el=e.currentTarget;setViewport(v=>({...v,scroll:el.scrollTop}));}}>
        {items.length===0?<div className="media-empty"><Icon name="image"/><p>{loading?t.loading:scan?t.empty:t.choose}</p>{!scan&&!loading&&<button onClick={()=>void pick()}>{t.folder}</button>}</div>:<div className="media-grid" role="listbox" aria-label={t.content} style={{gridTemplateColumns:`repeat(${columns},minmax(0,1fr))`,gridAutoRows:rowHeight-12,paddingTop:Math.floor(start/columns)*rowHeight+12,paddingBottom:Math.ceil((items.length-end)/columns)*rowHeight+12}}>{items.slice(start,end).map(item=><Thumbnail key={`${scan?.id}-${item.id}`} session={scan!.id} item={item} selected={selected?.id===item.id} onSelect={setSelected} unavailable={t.unavailable}/>)}</div>}
        {items.length>0&&items.length<total&&<button className="media-more" onClick={()=>void more()}>{t.more}</button>}
      </div>
    </section><aside className="media-preview"><h3>{t.preview}</h3>{selected&&scan?<Preview key={`${scan.id}-${selected.id}`} session={scan.id} item={selected} t={t} locale={locale}/>:<div className="media-empty"><Icon name="image"/><p>{t.select}</p></div>}</aside></div>
    <footer className="media-status"><span>{loading?t.loading:scan?.truncated?t.limit:t.ready}</span><label><Icon name="image"/><input aria-label={t.thumb} type="range" min="120" max="240" step="10" value={thumb} onChange={e=>{setThumb(Number(e.target.value));if(grid.current)grid.current.scrollTop=0;}}/></label></footer>
  </section>;
}
