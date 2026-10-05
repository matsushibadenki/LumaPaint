import { layerGroupLabels } from './layer-group-labels';
import { Fragment, useEffect, useRef, useState, type CSSProperties, type DragEvent, type PointerEvent, type ReactNode } from 'react';
import type { LayerSnapshot, TextObjectSnapshot, LayerGroup, LayerGroupsState, LayerGroupEdit } from '../bridge';
import type { Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { Icon } from './Icon';

type Gesture = { id: string; pointerId: number; startX: number; startY: number; x: number; y: number; dragging: boolean };

export function LayerList({ groups, onGroupEdit, thumbnails = {}, thumbnailError, layers, textObjects, selectedId, enabled, selectable = enabled, appearanceEnabled = enabled, reorderEnabled = enabled, locale, onSelect, onToggle, onToggleLock, onSelectObject, onToggleObject, onReorderObjects, selectedObjects, onRename, onReorder }: {
  groups:LayerGroupsState; onGroupEdit:(edit:LayerGroupEdit)=>void;
  thumbnails?: Record<string,string>; thumbnailError?: string;
  layers: LayerSnapshot[]; textObjects: TextObjectSnapshot[]; selectedId: string; enabled: boolean; locale: Locale;
  selectable?: boolean;
  appearanceEnabled?: boolean;
  reorderEnabled?: boolean;
  selectedObjects: string[]; onSelectObject: (layerId: string, objectId: string) => void;
  onToggleObject: (layerId: string, objectId: string, visible: boolean) => void;
  onReorderObjects: (layerId: string, ids: string[]) => void;
  onToggleLock: (layer: LayerSnapshot) => void;
  onSelect: (id: string) => void; onToggle: (id: string) => void;
  onRename: (layer: LayerSnapshot, name: string) => void; onReorder: (ids: string[]) => void;
}) {
  const t = workspaceMessages[locale];
  const list = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [gap, setGap] = useState<number | null>(null);
  const [folderDrop,setFolderDrop]=useState<string|null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(() => new Set());
  const [objectDrag, setObjectDrag] = useState<{ layerId: string; objectId: string; targetId: string } | null>(null);
  const textLayers = new Set(textObjects.map(object => object.layerId));
  const groupText = layerGroupLabels[locale];
  type Row = LayerSnapshot & {group?:LayerGroup;depth:number;parent?:string};
  const displayed:Row[] = [];
  const append = (ids:string[],depth:number,parent?:string) => {
    for (const id of ids) {
      const group = groups.groups.find(g=>g.id===id);
      if (group) {
        displayed.push({id:group.id,name:group.name,kind:'svg',visible:group.visible,locked:group.locked,opacity:1,alphaLocked:false,maskEnabled:false,maskInverted:false,maskDensity:1,deletable:true,strokeCount:0,objects:[],group,depth,parent});
        if (!group.collapsed) append(group.children,depth+1,group.id);
      } else {
        const layer=layers.find(l=>l.id===id);
        if(layer) displayed.push({...layer,depth,parent});
      }
    }
  };
  append(groups.roots,0);
  // Old projects and the fixed background have no folder metadata.
  for(const layer of [...layers].reverse()) if(!displayed.some(row=>row.id===layer.id)&&!groups.groups.some(g=>g.children.includes(layer.id))) displayed.push({...layer,depth:0});

  const orderKey = displayed.map(layer => layer.id).join('|');

  function cancel() {
    const current = gesture.current;
    gesture.current = null;
    if (current && list.current?.hasPointerCapture(current.pointerId)) list.current.releasePointerCapture(current.pointerId);
    setDraggedId(null);
    setGap(null);
    setFolderDrop(null);
  }

  // Changing documents, losing focus, or leaving the panel must never commit a drag.
  useEffect(() => {
    cancel();
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') cancel(); };
    window.addEventListener('keydown', escape);
    window.addEventListener('blur', cancel);
    return () => { cancel(); window.removeEventListener('keydown', escape); window.removeEventListener('blur', cancel); };
  }, [orderKey, enabled, reorderEnabled]);

  function hitGap(x: number, y: number): number | null {
    const element = list.current;
    if (!element) return null;
    const bounds = element.getBoundingClientRect();
    if (x < bounds.left || x > bounds.right || y < bounds.top || y > bounds.bottom) return null;
    const rows = [...element.querySelectorAll<HTMLElement>('[data-layer-id]')];
    const index = rows.findIndex(row => {
      const rect = row.getBoundingClientRect();
      return y < rect.top + rect.height / 2;
    });
    // The legacy base layer is fixed; the last valid gap is immediately above it.
    return Math.min(index < 0 ? rows.length : index, enabled ? displayed.filter(layer => layer.deletable).length : displayed.length);
  }

  useEffect(() => {
    if (!draggedId) return;
    let frame = 0;
    const scroll = () => {
      const current = gesture.current;
      const element = list.current;
      if (!current || !element) return;
      const rect = element.getBoundingClientRect();
      if (current.x >= rect.left && current.x <= rect.right && current.y >= rect.top && current.y <= rect.bottom) {
        const speed = current.y < rect.top + 28 ? -7 : current.y > rect.bottom - 28 ? 7 : 0;
        if (speed) { element.scrollTop += speed; setGap(hitGap(current.x, current.y)); }
      }
      frame = requestAnimationFrame(scroll);
    };
    frame = requestAnimationFrame(scroll);
    return () => cancelAnimationFrame(frame);
  }, [draggedId, orderKey]);

  function start(event: PointerEvent<HTMLDivElement>, layer: LayerSnapshot) {
    if (!reorderEnabled || event.button !== 0 || !event.isPrimary || gesture.current || (event.target as Element).closest('button, input')) return;
    if (!enabled) {
      onSelect(layer.id);
      event.currentTarget.focus({ preventScroll: true });
      gesture.current = { id: layer.id, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY, dragging: false };
      return;
    }
    if(event.shiftKey&&groups.selected.length) {
      const anchor=displayed.findIndex(row=>row.id===groups.selected[0]);
      const end=displayed.findIndex(row=>row.id===layer.id);
      if(anchor>=0&&end>=0)onGroupEdit({action:'select',id:layer.id,ids:displayed.slice(Math.min(anchor,end),Math.max(anchor,end)+1).map(row=>row.id)});
      return;
    }
    if (event.metaKey || event.ctrlKey) {
      onGroupEdit({action:'select',id:layer.id,additive:true});
      return;
    }
    if (!groups.selected.includes(layer.id)) onGroupEdit({action:'select',id:layer.id});
    event.currentTarget.focus({ preventScroll: true });
    if (!layer.deletable) return;
    gesture.current = { id: layer.id, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY, dragging: false };
  }

  function move(event: PointerEvent<HTMLDivElement>) {
    const current = gesture.current;
    if (!current || current.pointerId !== event.pointerId) return;
    current.x = event.clientX; current.y = event.clientY;
    if (!current.dragging && Math.hypot(current.x - current.startX, current.y - current.startY) < 5) return;
    event.preventDefault();
    if (!current.dragging) list.current?.setPointerCapture(event.pointerId);
    current.dragging = true;
    setDraggedId(current.id);
    setGap(hitGap(current.x, current.y));
    const hovered=[...list.current!.querySelectorAll<HTMLElement>('[data-layer-id]')].find(row=>{const r=row.getBoundingClientRect();return current.y>=r.top+8&&current.y<=r.bottom-8&&current.x>=r.left&&current.x<=r.right;});
    const id=hovered?.dataset.layerId;
    setFolderDrop(id&&id!==current.id&&groups.groups.some(g=>g.id===id)?id:null);
  }

  function finish(event: PointerEvent<HTMLDivElement>) {
    const current = gesture.current;
    if (!current || current.pointerId !== event.pointerId) return;
    const insertion = hitGap(event.clientX, event.clientY);
    if (reorderEnabled && current.dragging && insertion !== null) {
      if (!enabled) {
        const ids = displayed.map(layer => layer.id);
        const from = ids.indexOf(current.id);
        if (from >= 0) {
          const next = ids.filter(id => id !== current.id);
          next.splice(insertion - (from < insertion ? 1 : 0), 0, current.id);
          if (next.some((id, i) => id !== ids[i])) onReorder(next);
        }
        cancel();
        return;
      }
      const rows=[...list.current!.querySelectorAll<HTMLElement>('[data-layer-id]')];
      const hover=rows.find(row=>{const r=row.getBoundingClientRect();return event.clientY>=r.top+8&&event.clientY<=r.bottom-8;});
      const folder=hover&&groups.groups.find(g=>g.id===hover.dataset.layerId);
      const source=displayed.find(row=>row.id===current.id);
      const selected=groups.selected.includes(current.id)?groups.selected.filter(id=>id!=='layer-1'):[current.id];
      if(folder&&!selected.includes(folder.id)) onGroupEdit({action:'move',ids:selected,target:folder.id});
      else if(source) {
        const targetRow=displayed[insertion];
        const parent=targetRow?.parent;
        if(source.parent!==parent) onGroupEdit({action:'move',ids:selected,target:parent??''});
        else {
          const siblings=parent?groups.groups.find(g=>g.id===parent)!.children:groups.roots;
          const next=siblings.filter(id=>!selected.includes(id));
          const target=targetRow?next.indexOf(targetRow.id):-1;
          next.splice(target<0?next.length:target,0,...siblings.filter(id=>selected.includes(id)));
          if(next.some((id,index)=>id!==siblings[index])) onGroupEdit({action:'reorder',ids:next,target:parent??''});
        }
      }
    }
    cancel();
  }

  function commitName(layer: LayerSnapshot) {
    const trimmed = name.trim();
    setEditingId(null);
    if (trimmed && trimmed !== layer.name) {
      if(groups.groups.some(g=>g.id===layer.id)) onGroupEdit({action:'rename',id:layer.id,name:trimmed});
      else onRename(layer, trimmed);
    }
  }

  function dropObject(event: DragEvent<HTMLDivElement>, layer: LayerSnapshot, targetId: string) {
    event.preventDefault();
    if (!objectDrag || objectDrag.layerId !== layer.id) return;
    const ids = [...layer.objects].reverse().map(object => object.id);
    const from = ids.indexOf(objectDrag.objectId);
    const target = ids.indexOf(targetId);
    if (from < 0 || target < 0) return;
    const next = [...ids];
    next.splice(from, 1);
    next.splice(target, 0, objectDrag.objectId);
    setObjectDrag(null);
    if (next.some((id, index) => id !== ids[index])) onReorderObjects(layer.id, next);
  }

  function renderObjects(layer: LayerSnapshot) {
    const rows: ReactNode[] = [];
    let previousPath: string[] = [];
    for (const object of [...layer.objects].reverse()) {
      let shared = 0;
      while (shared < previousPath.length && shared < object.groupPath.length && previousPath[shared] === object.groupPath[shared]) shared += 1;
      for (let depth = shared; depth < object.groupPath.length; depth += 1) {
        if (object.groupPath.slice(0, depth).some(groupId => collapsedGroups.has(`${layer.id}:${groupId}`))) break;
        const groupId = object.groupPath[depth];
        const key = `${layer.id}:${groupId}`;
        const collapsed = collapsedGroups.has(key);
        rows.push(<button type="button" className="layer-group-row" style={{ '--group-depth': depth } as CSSProperties} key={`group:${key}`}
          aria-expanded={!collapsed} onClick={() => setCollapsedGroups(current => { const next = new Set(current); if (next.has(key)) next.delete(key); else next.add(key); return next; })}>
          <Icon name={collapsed ? 'chevronRight' : 'chevronDown'} /><span>{t.vectorGroup}</span>
        </button>);
      }
      previousPath = object.groupPath;
      if (object.groupPath.some(groupId => collapsedGroups.has(`${layer.id}:${groupId}`))) continue;
      rows.push(<div className={`layer-object-row${objectDrag?.targetId === object.id ? ' object-drop-target' : ''}`} style={{ '--group-depth': object.groupPath.length } as CSSProperties} role="listitem" key={object.id}
        draggable={enabled && !layer.locked && !object.locked && object.groupPath.length === 0} onDragStart={event => {
          event.stopPropagation();
          event.dataTransfer.effectAllowed = 'move';
          event.dataTransfer.setData('text/plain', object.id);
          setObjectDrag({ layerId: layer.id, objectId: object.id, targetId: object.id });
        }} onDragOver={event => {
          if (objectDrag?.layerId !== layer.id || object.groupPath.length > 0) return;
          event.preventDefault(); event.dataTransfer.dropEffect = 'move';
          if (objectDrag.targetId !== object.id) setObjectDrag({ ...objectDrag, targetId: object.id });
        }} onDrop={event => dropObject(event, layer, object.id)} onDragEnd={() => setObjectDrag(null)}>
        <button type="button" className="icon-button object-visibility" disabled={!enabled || layer.locked || !layer.visible || object.locked}
          title={object.visible ? t.hideObject : t.showObject} aria-label={`${object.visible ? t.hideObject : t.showObject}: ${object.name}`}
          aria-pressed={object.visible} onClick={() => onToggleObject(layer.id, object.id, !object.visible)}>
          <Icon name={object.visible ? 'eye' : 'eyeOff'} />
        </button>
        <button type="button" className="layer-object-select" disabled={!enabled || layer.locked || !layer.visible || !object.visible || object.locked}
          aria-pressed={selectedObjects.includes(object.id)} onClick={() => onSelectObject(layer.id, object.id)}>
          <Icon name={object.imageFrame ? (object.kind === 'ellipse' ? 'imageFrameEllipse' : 'imageFrameRectangle') : object.kind === 'text' ? 'text' : object.kind === 'rectangle' ? 'vectorRectangle' : object.kind === 'ellipse' ? 'vectorEllipse' : 'vector'} />
          <span>{object.name}</span>
          {object.locked && <span className="object-lock-indicator" title={t.lockLayer}><Icon name="lock" /></span>}
          <span className="object-target" aria-hidden="true" />
        </button>
      </div>);
    }
    return rows;
  }

  return <div ref={list} className={`layer-list${draggedId ? ' is-dragging' : ''}`} role="list" aria-label={t.layers}
    onPointerMove={move} onPointerUp={finish} onPointerCancel={cancel} onLostPointerCapture={cancel}
    onPointerLeave={() => { if (!gesture.current?.dragging) cancel(); }}
    onDragStart={event => { if (!(event.target as Element).closest('.layer-object-row')) event.preventDefault(); }}>
    {displayed.map((layer, index) => {
      if(layer.group) {
        const group=layer.group;
        return <div key={group.id} data-layer-id={group.id} role="listitem" tabIndex={enabled?0:-1}
          className={`layer-row layer-folder-row${groups.selected.includes(group.id)?' selected':''}${draggedId===group.id?' dragging':''}${folderDrop===group.id?' folder-drop-target':''}${gap===index?' insert-before':''}`}
          style={{paddingLeft:6}} data-movable={enabled} onPointerDown={event=>start(event,layer)}
          onKeyDown={event=>{if(event.target!==event.currentTarget||!enabled)return;if(event.key==='Enter'||event.key===' '){event.preventDefault();onGroupEdit({action:'select',id:group.id,additive:event.metaKey||event.ctrlKey});}if(event.key==='ArrowLeft'&&!group.collapsed||event.key==='ArrowRight'&&group.collapsed){event.preventDefault();onGroupEdit({action:'collapse',id:group.id});}}}>
          <div className="layer-indicators"><button className="icon-button" disabled={!enabled} aria-label={`${group.visible?t.hideLayer:t.showLayer}: ${group.name}`} aria-pressed={group.visible} onClick={()=>onGroupEdit({action:'visibility',id:group.id})}><Icon name={group.visible?'eye':'eyeOff'}/></button></div>
          <span className="layer-color-spacer" aria-hidden="true" />
          <div className="layer-row-content" style={{marginLeft:Math.min(layer.depth,4)*24}}>
          <button className="icon-button layer-folder-disclosure" disabled={!enabled} aria-label={`${group.collapsed?groupText.expand:groupText.collapse}: ${group.name}`} aria-expanded={!group.collapsed} onClick={()=>onGroupEdit({action:'collapse',id:group.id})}><Icon name={group.collapsed?'chevronRight':'chevronDown'}/></button>
          <span className="layer-folder-icon"><Icon name="folder"/></span>
          {group.maskEnabled&&<span className={`mask-thumb${group.maskInverted?' inverted':''}`} style={{backgroundColor:group.maskInverted?`rgb(${Math.round(255*(1-group.maskDensity))} ${Math.round(255*(1-group.maskDensity))} ${Math.round(255*(1-group.maskDensity))})`:'#fff'}} aria-hidden="true"/>}
          <div className="layer-description">{editingId===group.id?<input className="layer-name" autoFocus maxLength={120} aria-label={groupText.rename} value={name} onFocus={event=>event.currentTarget.select()} onChange={event=>setName(event.target.value)} onBlur={()=>commitName(layer)} onKeyDown={event=>{if(event.key==='Enter')commitName(layer);if(event.key==='Escape')setEditingId(null);}}/>:<span className="layer-name" onDoubleClick={()=>{if(enabled){setName(group.name);setEditingId(group.id);}}}>{group.name}</span>}</div>
          </div>
          <button className="icon-button layer-lock-button" disabled={!enabled} title={group.locked?t.unlockLayer:t.lockLayer} aria-label={`${group.locked?t.unlockLayer:t.lockLayer}: ${group.name}`} aria-pressed={group.locked} onClick={()=>onGroupEdit({action:'lock',id:group.id})}><Icon name={group.locked?'lock':'unlock'}/></button>
        </div>;
      }

      const type = layer.kind === 'paint' ? 'pixel' : layer.kind === 'svg' ? 'image' : textLayers.has(layer.id) ? 'textVector' : 'vector';
      const label = t.layerTypes[type];
      const displayName = !layer.deletable && ['Layer 1', 'Layer1', 'Background'].includes(layer.name) ? t.backgroundLayer : layer.name;
      const objects = layer.objects ?? [];
      const isExpanded = expanded.has(layer.id);
      return <Fragment key={layer.id}><div data-layer-id={layer.id} role="listitem" tabIndex={selectable ? 0 : -1}
      className={`layer-row${groups.selected.includes(layer.id) || (groups.selected.length===0&&selectedId === layer.id) ? ' selected' : ''}${draggedId === layer.id ? ' dragging' : ''}${gap === index ? ' insert-before' : ''}`}
      style={{paddingLeft:6}} data-movable={reorderEnabled && (!enabled || layer.deletable)} title={reorderEnabled && (!enabled || layer.deletable) ? t.reorderLayer : t.fixedBaseLayer}
      onPointerDown={event => { if (!reorderEnabled && selectable && event.button === 0 && !(event.target as Element).closest('input, button')) onSelect(layer.id); else start(event, layer); }}
      onKeyDown={event => {
        if (!selectable || (event.target as Element).closest('input, button')) return;
        if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onSelect(layer.id); }
        if (!enabled && reorderEnabled && event.altKey && ['ArrowUp', 'ArrowDown'].includes(event.key)) {
          event.preventDefault();
          const ids = displayed.map(row => row.id);
          const from = ids.indexOf(layer.id);
          const to = from + (event.key === 'ArrowUp' ? -1 : 1);
          if (to >= 0 && to < ids.length) { ids.splice(from, 1); ids.splice(to, 0, layer.id); onReorder(ids); }
          return;
        }
        if (!enabled) return;
        if (!event.altKey || !['ArrowUp', 'ArrowDown'].includes(event.key) || !layer.deletable) return;
        event.preventDefault();
        const ids=[...(layer.parent?groups.groups.find(g=>g.id===layer.parent)!.children:groups.roots)];
        const position=ids.indexOf(layer.id);
        const target=position+(event.key==='ArrowUp'?-1:1);
        if(target>=0&&target<ids.length){ids.splice(position,1);ids.splice(target,0,layer.id);onGroupEdit({action:'reorder',ids,target:layer.parent??''});}
      }}>
      <div className="layer-indicators">
      <button className="icon-button" disabled={!appearanceEnabled} onClick={() => onToggle(layer.id)} title={layer.visible ? t.hideLayer : t.showLayer}
        aria-label={`${layer.visible ? t.hideLayer : t.showLayer}: ${displayName}`} aria-pressed={layer.visible}><Icon name={layer.visible ? 'eye' : 'eyeOff'} /></button>
      </div>
      <span className={`path-color-stripe${layer.kind === 'vector' ? '' : ' pixel-color-stripe'}`} aria-hidden="true" style={layer.kind === 'vector' ? { backgroundColor: `rgb(${(layer.guideColor ?? [48,144,255]).slice(0,3).join(',')})` } : undefined} />
      <div className="layer-row-content" style={{marginLeft:Math.min(layer.depth,4)*24}}>
      <span className={`layer-thumb ${layer.kind}`} aria-hidden="true" title={thumbnailError}>{thumbnails[layer.id] && <img src={thumbnails[layer.id]} alt="" draggable={false} />}</span>
      {layer.maskEnabled && <span className={`mask-thumb${layer.maskInverted ? ' inverted' : ''}`} style={{backgroundColor:layer.maskInverted?`rgb(${Math.round(255*(1-layer.maskDensity))} ${Math.round(255*(1-layer.maskDensity))} ${Math.round(255*(1-layer.maskDensity))})`:'#fff'}} aria-hidden="true" />}
      <div className="layer-description">
      {editingId === layer.id ? <input className="layer-name" autoFocus maxLength={120} aria-label={t.renameLayer} value={name}
        onFocus={event => event.currentTarget.select()} onChange={event => setName(event.target.value)}
        onBlur={() => commitName(layer)} onKeyDown={event => { if (event.key === 'Enter') commitName(layer); if (event.key === 'Escape') setEditingId(null); }} />
        : <span className="layer-name" onDoubleClick={() => { if (appearanceEnabled) { setName(displayName); setEditingId(layer.id); } }}>{displayName}</span>}
      <span className="layer-type-label">{label}</span>
      </div>
      {layer.alphaLocked && <span className="layer-lock" title={t.lockAlpha}>α</span>}
      <button type="button" className="icon-button layer-disclosure" disabled={objects.length === 0} aria-expanded={isExpanded}
        aria-label={`${isExpanded ? t.collapseLayer : t.expandLayer}: ${displayName}`} title={isExpanded ? t.collapseLayer : t.expandLayer}
        onClick={() => setExpanded(current => { const next = new Set(current); if (next.has(layer.id)) next.delete(layer.id); else next.add(layer.id); return next; })}>
        <Icon name={isExpanded ? 'chevronDown' : 'chevronRight'} />
      </button>
      </div>
      <button type="button" className="icon-button layer-lock-button" disabled={!appearanceEnabled}
        title={layer.locked ? t.unlockLayer : t.lockLayer}
        aria-label={`${layer.locked ? t.unlockLayer : t.lockLayer}: ${displayName}`}
        aria-pressed={layer.locked} onClick={() => onToggleLock(layer)}><Icon name={layer.locked ? 'lock' : 'unlock'} /></button>
    </div>
    {isExpanded && objects.length > 0 && <div className="layer-children" role="list" aria-label={displayName}>
      {renderObjects(layer)}
    </div>}
    </Fragment>; })}
  </div>;
}
