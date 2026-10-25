/** Presentation only: IDs refer to Rust-owned documents; no document data lives here. */
export type Pane = { tabs: number[]; selected: number | null };
export type DocumentLayout = { rows: number; columns: number; panes: Pane[]; focused: number; rowSizes: number[]; columnSizes: number[] };
export type DropEdge = 'left' | 'right' | 'top' | 'bottom' | 'center';
export const emptyLayout = (): DocumentLayout => ({rows:1,columns:1,focused:0,rowSizes:[1],columnSizes:[1],panes:[{tabs:[],selected:null}]});
export function reconcileLayout(layout: DocumentLayout, ids: number[], active: number | null): DocumentLayout {
  if (!ids.length) return emptyLayout();
  const panes=layout.panes.map(p=>({tabs:p.tabs.filter(id=>ids.includes(id)),selected:p.selected}));
  const found=new Set(panes.flatMap(p=>p.tabs));
  for(const id of ids) if(!found.has(id)) {
    const pane=panes.find(p=>!p.tabs.length)??panes[layout.focused]??panes[0];
    pane.tabs.push(id); pane.selected=id; found.add(id);
  }
  for(const pane of panes) pane.selected=active!==null&&pane.tabs.includes(active)?active:pane.tabs.includes(pane.selected??-1)?pane.selected:pane.tabs[0]??null;
  return {...layout,panes,focused:Math.max(0,panes.findIndex(p=>active!==null&&p.tabs.includes(active)))};
}
export function arrangeLayout(layout:DocumentLayout,rows:number,columns:number):DocumentLayout {
  if(![1,2,3].includes(rows)||![1,2,3].includes(columns)) return layout;
  const ids=layout.panes.flatMap(p=>p.tabs);
  const panes=Array.from({length:rows*columns},()=>({tabs:[] as number[],selected:null as number|null}));
  ids.forEach((id,i)=>{const p=panes[i<panes.length?i:0];p.tabs.push(id);p.selected??=id;});
  return {rows,columns,panes,focused:0,rowSizes:Array(rows).fill(1/rows),columnSizes:Array(columns).fill(1/columns)};
}
export function dropDocument(layout:DocumentLayout,id:number,target:number,edge:DropEdge):DocumentLayout {
  if(!layout.panes.some(p=>p.tabs.includes(id))||!layout.panes[target]) return layout;
  const result={...layout,panes:layout.panes.map(p=>({tabs:p.tabs.filter(n=>n!==id),selected:p.selected}))};
  let row=Math.floor(target/layout.columns),column=target%layout.columns;
  if((edge==='left'||edge==='right')&&layout.columns<3){
    const sizes=[...layout.columnSizes],weight=sizes[column];sizes.splice(column,1,weight/2,weight/2);result.columnSizes=sizes;
    column+=edge==='right'?1:0;result.columns++;
    result.panes=Array.from({length:result.rows*result.columns},(_,i)=>{
      const r=Math.floor(i/result.columns),c=i%result.columns;
      return c===column?{tabs:[],selected:null}:result.panes[r*layout.columns+c-(c>column?1:0)];
    });
  }else if((edge==='top'||edge==='bottom')&&layout.rows<3){
    const sizes=[...layout.rowSizes],weight=sizes[row];sizes.splice(row,1,weight/2,weight/2);result.rowSizes=sizes;
    row+=edge==='bottom'?1:0;result.rows++;
    result.panes=Array.from({length:result.rows*result.columns},(_,i)=>{
      const r=Math.floor(i/result.columns),c=i%result.columns;
      return r===row?{tabs:[],selected:null}:result.panes[(r-(r>row?1:0))*result.columns+c];
    });
  }
  result.focused=row*result.columns+column;
  const pane=result.panes[result.focused];pane.tabs.push(id);pane.selected=id;
  for(const p of result.panes) if(!p.tabs.includes(p.selected??-1))p.selected=p.tabs[0]??null;
  return result;
}

/** Resize the adjacent pair only; preserve the pair sum and all other tracks. */
export function resizeLayout(layout:DocumentLayout,axis:'columns'|'rows',index:number,delta:number,minimum:number):DocumentLayout {
 const sizes=axis==='columns'?layout.columnSizes:layout.rowSizes;
 if(index<0||index+1>=sizes.length||!Number.isFinite(delta)||!Number.isFinite(minimum))return layout;
 const next=[...sizes],sum=sizes[index]+sizes[index+1],min=Math.min(sum/2,Math.max(.001,minimum));
 next[index]=Math.max(min,Math.min(sum-min,sizes[index]+delta));next[index+1]=sum-next[index];
 return {...layout,[axis==='columns'?'columnSizes':'rowSizes']:next};
}
