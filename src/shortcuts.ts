import {useLayoutEffect,useSyncExternalStore} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
export type ShortcutCommand={id:string;label:string;category:string;defaultKey:string};
export type Command=ShortcutCommand & {enabled?:boolean;action:()=>void};
export type ShortcutSet={id:string;name:string;bindings:Record<string,string>};
export type ShortcutSettings={version:number;revision:number;active:string;sets:ShortcutSet[]};
export type ShortcutSnapshot={settings:ShortcutSettings;commands:ShortcutCommand[]};
const initial:ShortcutSettings={version:1,revision:0,active:'default',sets:[{id:'default',name:'LumaPaint',bindings:{}}]};
let snapshot:ShortcutSnapshot={settings:initial,commands:[]};
const groups=new Map<string,Command[]>(),subscribers=new Set<()=>void>();
let inputBlocked=false;
let initialized=false,timer:ReturnType<typeof setTimeout>|undefined;
const notify=()=>{for(const f of subscribers)f();};
export const binding=(settings:ShortcutSettings,command:ShortcutCommand)=>settings.sets.find(s=>s.id===settings.active)?.bindings[command.id]??command.defaultKey;
const symbols:Record<string,string>={BracketLeft:'[',BracketRight:']',Backslash:'\\',Semicolon:';',Quote:"'",Comma:',',Period:'.',Slash:'/',Minus:'-',Equal:'=',Backquote:'`',IntlYen:'¥'};
export function keyFromAccelerator(value:string){const parts=value.split('+'),last=parts.pop()??'';const code=/^[A-Z]$/i.test(last)?`Key${last.toUpperCase()}`:/^[0-9]$/.test(last)?`Digit${last}`:Object.keys(symbols).find(k=>symbols[k]===last)??last;return [...(parts.includes('CmdOrCtrl')?['Primary']:[]),...(parts.includes('Ctrl')?['Control']:[]),...(parts.includes('Alt')?['Alt']:[]),...(parts.includes('Shift')?['Shift']:[]),code].join('+');}
export function accelerator(key:string){return key.split('+').map(k=>k==='Primary'?'CmdOrCtrl':k==='Control'?'Ctrl':symbols[k]??k.replace(/^Key|^Digit/,'')).join('+');}
export function displayKey(key:string){const mac=/Mac/.test(navigator.platform);return key.split('+').map(k=>k==='Primary'?(mac?'⌘':'Ctrl'):k==='Control'?(mac?'⌃':'Ctrl'):k==='Alt'?(mac?'⌥':'Alt'):k==='Shift'?(mac?'⇧':'Shift'):symbols[k]??k.replace(/^Key|^Digit/,'')).join(mac?'':'+');}
export function eventKey(e:Pick<KeyboardEvent,'code'|'metaKey'|'ctrlKey'|'altKey'|'shiftKey'>){const mac=/Mac/.test(navigator.platform);return [...((mac?e.metaKey:e.ctrlKey)?['Primary']:[]),...(mac&&e.ctrlKey?['Control']:[]),...(e.altKey?['Alt']:[]),...(e.shiftKey?['Shift']:[]),e.code].join('+');}
export function validKey(key:string){if(!key)return true;const code=key.split('+').at(-1)??'';return /^(Key[A-Z]|Digit[0-9]|F([1-9]|1[0-9]|20)|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|Minus|Equal|Backquote|IntlYen)$/.test(code)&&!['Primary+KeyQ','Primary+KeyH','Primary+Alt+KeyH','Primary+KeyM'].includes(key);}
function run(id:string,native=false){if(inputBlocked)return;if(!native&&document.activeElement instanceof HTMLElement&&document.activeElement.closest('.layer-panel')){if(id==='menu.group')id='layerGroup.group';else if(id==='menu.ungroup')id='layerGroup.ungroup';}const command=[...groups.values()].flat().find(c=>c.id===id);if(command?.enabled!==false)command?.action();}
async function init(){if(initialized)return;initialized=true;if(isTauri()){await listen<string>('shortcut-command',e=>run(e.payload,true));await listen<ShortcutSettings>('shortcuts-changed',e=>{if(e.payload.revision>=snapshot.settings.revision){snapshot={...snapshot,settings:e.payload};notify();}});}else{try{const saved=localStorage.getItem('lumapaint-shortcuts-preview');if(saved)snapshot={...snapshot,settings:JSON.parse(saved)};}catch{/* preview-only storage */}}
 window.addEventListener('keydown',e=>{if(inputBlocked||document.querySelector('dialog[open]')||e.defaultPrevented||e.isComposing||e.repeat||e.target instanceof HTMLElement&&e.target.closest('input,textarea,select,[contenteditable=true],dialog[open]'))return;const command=snapshot.commands.find(c=>binding(snapshot.settings,c)===eventKey(e));if(command){e.preventDefault();e.stopImmediatePropagation();run(command.id);}},true);
}
function register(group:string,commands:Command[]){groups.set(group,commands);const unique=new Map([...groups.values()].flat().map(c=>[c.id,c]));const metadata=[...unique.values()].map(({id,label,category,defaultKey})=>({id,label,category,defaultKey}));if(JSON.stringify(metadata)===JSON.stringify(snapshot.commands))return;snapshot={...snapshot,commands:metadata};notify();clearTimeout(timer);timer=setTimeout(()=>{void init().then(async()=>{if(isTauri()){const result=await invoke<ShortcutSnapshot>('shortcut_catalog',{commands:metadata});if(result.settings.revision>=snapshot.settings.revision){snapshot={...snapshot,settings:result.settings};notify();}}}).catch(e=>console.error(e));},0);}
export function useShortcutCommands(group:string,commands:Command[]){useLayoutEffect(()=>{register(group,commands);});useLayoutEffect(()=>()=>{groups.delete(group);},[group]);}
export function useShortcuts(){return useSyncExternalStore(f=>{subscribers.add(f);return()=>{subscribers.delete(f);};},()=>snapshot);}
export async function readShortcuts():Promise<ShortcutSnapshot>{return isTauri()?invoke('shortcut_catalog',{commands:null}):structuredClone(snapshot);}
export async function saveShortcuts(settings:ShortcutSettings){const result=isTauri()?await invoke<ShortcutSettings>('save_shortcuts',{settings}):{...settings,revision:settings.revision+1};if(!isTauri())localStorage.setItem('lumapaint-shortcuts-preview',JSON.stringify(result));snapshot={...snapshot,settings:result};notify();return result;}
export function commandShortcut(id:string){const command=snapshot.commands.find(c=>c.id===id);return command?displayKey(binding(snapshot.settings,command)):'';}

export function useShortcutInputBlocked(blocked:boolean){useLayoutEffect(()=>{inputBlocked=blocked;return()=>{inputBlocked=false;};},[blocked]);}
