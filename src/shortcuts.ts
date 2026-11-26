import {propertyKeysOwnKey} from './timeline-key-group';
import {graphOwnsKey} from './timeline-graph';
import {useLayoutEffect,useSyncExternalStore} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
export type ShortcutCommand={id:string;label:string;category:string;defaultKey:string};
export type Command=ShortcutCommand & {enabled?:boolean;disabledReason?:string;shortcutEnabled?:boolean;action:()=>void};
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
const commandsById = new Map<string, Command>();
const availabilitySubscribers = new Set<() => void>();
let unavailable = new Map<string, string>();

export function useCommandAvailability() {
  return useSyncExternalStore(listener => {
    availabilitySubscribers.add(listener);
    return () => { availabilitySubscribers.delete(listener); };
  }, () => unavailable);
}

export function executeCommand(id: string, source: 'palette' | 'native' | 'web' = 'palette') {
  if (inputBlocked || (source !== 'palette' && document.querySelector('dialog[open]'))) return;
  if (source === 'web' && document.activeElement instanceof HTMLElement && document.activeElement.closest('.layer-panel')) {
    if (id === 'menu.group') id = 'layerGroup.group';
    else if (id === 'menu.ungroup') id = 'layerGroup.ungroup';
  }
  const command = commandsById.get(id);
  if (!command) return;
  const enabled = source === 'palette' ? command.enabled : command.shortcutEnabled ?? command.enabled;
  if (enabled !== false) command.action();
}
async function init(){if(initialized)return;initialized=true;if(isTauri()){await listen<string>('shortcut-command',e=>executeCommand(e.payload,'native'));await listen<ShortcutSettings>('shortcuts-changed',e=>{if(e.payload.revision>=snapshot.settings.revision){snapshot={...snapshot,settings:e.payload};notify();}});}else{try{const saved=localStorage.getItem('lumapaint-shortcuts-preview');if(saved)snapshot={...snapshot,settings:JSON.parse(saved)};}catch{/* preview-only storage */}}
 window.addEventListener('keydown',e=>{if(e.target instanceof Element&&e.target.closest('[data-shortcut-scope=timeline-property-keys]')&&propertyKeysOwnKey(e.key,e.metaKey||e.ctrlKey,e.altKey))return;if(e.target instanceof Element&&e.target.closest('[data-shortcut-scope=timeline-graph]')&&!e.target.closest('button,input,textarea,select,[contenteditable=true]')&&graphOwnsKey(e.key,e.metaKey||e.ctrlKey,e.altKey))return;if(inputBlocked||document.querySelector('dialog[open]')||e.defaultPrevented||e.isComposing||e.repeat||e.target instanceof HTMLElement&&e.target.closest('input,textarea,select,[contenteditable=true],dialog[open],[role=menu]'))return;const command=snapshot.commands.find(c=>binding(snapshot.settings,c)===eventKey(e));if(command){e.preventDefault();e.stopImmediatePropagation();executeCommand(command.id,'web');}},true);
}
function refreshCommands(){
 commandsById.clear();
 for (const commands of groups.values()) for (const command of commands) commandsById.set(command.id, command);
 const nextUnavailable = new Map([...commandsById.values()].filter(command => command.enabled === false).map(command => [command.id, command.disabledReason ?? ''] as const));
 if (unavailable.size !== nextUnavailable.size || [...unavailable].some(([id, reason]) => nextUnavailable.get(id) !== reason)) {
   unavailable = nextUnavailable;
   for (const listener of availabilitySubscribers) listener();
 }
 const metadata=[...commandsById.values()].map(({id,label,category,defaultKey})=>({id,label,category,defaultKey}));if(JSON.stringify(metadata)===JSON.stringify(snapshot.commands))return;snapshot={...snapshot,commands:metadata};notify();clearTimeout(timer);timer=setTimeout(()=>{void init().then(async()=>{if(isTauri()){const result=await invoke<ShortcutSnapshot>('shortcut_catalog',{commands:metadata});if(result.settings.revision>=snapshot.settings.revision){snapshot={...snapshot,settings:result.settings};notify();}}}).catch(e=>console.error(e));},0);}
export function useShortcutCommands(group:string,commands:Command[]){useLayoutEffect(()=>{groups.set(group,commands);refreshCommands();});useLayoutEffect(()=>()=>{groups.delete(group);refreshCommands();},[group]);}
export function useShortcuts(){return useSyncExternalStore(f=>{subscribers.add(f);return()=>{subscribers.delete(f);};},()=>snapshot);}
export async function readShortcuts():Promise<ShortcutSnapshot>{return isTauri()?invoke('shortcut_catalog',{commands:null}):structuredClone(snapshot);}
export async function saveShortcuts(settings:ShortcutSettings){const result=isTauri()?await invoke<ShortcutSettings>('save_shortcuts',{settings}):{...settings,revision:settings.revision+1};if(!isTauri())localStorage.setItem('lumapaint-shortcuts-preview',JSON.stringify(result));snapshot={...snapshot,settings:result};notify();return result;}
export function commandShortcut(id:string){const command=snapshot.commands.find(c=>c.id===id);return command?displayKey(binding(snapshot.settings,command)):'';}

export function useShortcutInputBlocked(blocked:boolean){useLayoutEffect(()=>{inputBlocked=blocked;return()=>{inputBlocked=false;};},[blocked]);}
