import {useEffect,useRef,useState} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {getCurrentWebviewWindow} from '@tauri-apps/api/webviewWindow';
import type {DocumentLayout} from './document-layout';
import type {DocumentWorkspaceSnapshot} from './bridge';
type Restored={layout:DocumentLayout|null;workspace:DocumentWorkspaceSnapshot|null;warnings:number};
let restoration:Promise<Restored>|undefined;
/** Commands carry only layout IDs and scalar view state. Rust owns files and documents. */
export function useWorkspaceSession(layout:DocumentLayout,onLayout:(layout:DocumentLayout)=>void,onWorkspace:(workspace:DocumentWorkspaceSnapshot)=>void,paused:boolean){
 const [ready,setReady]=useState(!isTauri()),[warning,setWarning]=useState(false);
 const latest=useRef(layout),blocked=useRef(paused);latest.current=layout;blocked.current=paused;
 const signature=JSON.stringify(layout);
 useEffect(()=>{if(!isTauri())return;let live=true;restoration??=invoke<Restored>('restore_document_workspace');void restoration.then(value=>{if(!live)return;if(value.workspace)onWorkspace(value.workspace);if(value.layout)onLayout(value.layout);setWarning(value.warnings>0);setReady(true);}).catch(()=>{if(live){setWarning(true);setReady(true);}});return()=>{live=false;};},[onLayout,onWorkspace]);
 useEffect(()=>{if(!ready||!isTauri()||paused)return;void invoke('save_document_workspace_view',{layout:latest.current}).catch(()=>setWarning(true));},[signature,ready,paused]);
 useEffect(()=>{if(!ready||!isTauri())return;let live=true,timer=0,stop=()=>{};void getCurrentWebviewWindow().listen('canvas-ruler-changed',()=>{clearTimeout(timer);timer=window.setTimeout(()=>{if(live&&!blocked.current)void invoke('save_document_workspace_view',{layout:latest.current}).catch(()=>{if(live)setWarning(true);});},250);}).then(fn=>{if(live)stop=fn;else fn();});return()=>{live=false;clearTimeout(timer);stop();};},[ready]);
 return warning;
}
