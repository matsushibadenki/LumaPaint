import type {CanvasTool} from './bridge';
import {workspaceMessages} from './workspace-i18n';
import {shortcutLabels} from './shortcut-i18n';
import {keyFromAccelerator,type Command} from './shortcuts';
import type {Locale} from './i18n';
const keys:Record<CanvasTool,string>={brush:'B',eraser:'E',blur:'Shift+R',sharpen:'Alt+Shift+R',smudge:'Alt+R',cloneStamp:'S',paintBucket:'Shift+G',lasso:'L',polygonLasso:'Shift+L',magneticLasso:'Alt+L',selectionBrush:'Shift+W',eyedropper:'I',gradient:'G',crop:'C',rectangle:'M',ellipse:'Shift+M',vectorSelect:'V',vectorDirectSelect:'A',vectorScale:'Shift+S',vectorRotate:'R',vectorPen:'P',vectorPencil:'N',vectorAnchorAdd:'Shift+Equal',vectorAnchorDelete:'Minus',vectorAnchorConvert:'Shift+C',vectorRectangle:'U',vectorEllipse:'Shift+U',imageFrameRectangle:'F',imageFrameEllipse:'Shift+F',text:'T',textVertical:'Shift+T',textFrame:'Alt+T',textFrameVertical:'Alt+Shift+T',zoomIn:'Z',zoomOut:'Alt+Z',hand:'H'};
export function toolCommands(locale:Locale,enabled:boolean,onTool:(tool:CanvasTool)=>void):Command[]{const labels=workspaceMessages[locale];return(Object.entries(keys) as [CanvasTool,string][]).map(([tool,key])=>({id:`tool.${tool}`,label:String(labels[tool as keyof typeof labels]??tool),category:shortcutLabels[locale].tools,defaultKey:keyFromAccelerator(key),enabled,action:()=>onTool(tool)}));}
