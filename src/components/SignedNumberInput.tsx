import type { KeyboardEvent } from 'react';

/** Parse only complete, finite decimal values. In-progress text remains editable. */
export function parseSignedNumber(text:string):number|null {
  if (!/^-?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(text)) return null;
  const value=Number(text);
  return Number.isFinite(value)?value:null;
}

export function SignedNumberInput({label,value,onChange,onBlur,onKeyDown,disabled,min,max,placeholder,allowEmpty=false,stepBase=0}:{
  label:string;value:string;onChange:(value:string)=>void;onBlur?:()=>void;onKeyDown?:(event:KeyboardEvent<HTMLInputElement>)=>void;
  disabled?:boolean;min?:number;max?:number;placeholder?:string;allowEmpty?:boolean;stepBase?:number;
}) {
  const number=parseSignedNumber(value);
  const valid=number!==null&&(min===undefined||number>=min)&&(max===undefined||number<=max);
  return <input type="text" inputMode="decimal" role="spinbutton" aria-label={label} aria-valuemin={min} aria-valuemax={max}
    aria-valuenow={valid?number:undefined} aria-invalid={!valid&&!(allowEmpty&&value==='')} value={value} disabled={disabled} placeholder={placeholder}
    onChange={event=>onChange(event.currentTarget.value)} onBlur={onBlur} onKeyDown={event=>{
      if(event.key==='ArrowUp'||event.key==='ArrowDown'){
        event.preventDefault();const next=(valid?number:stepBase)+(event.key==='ArrowUp'?1:-1);
        const bounded=Math.max(min??-Infinity,Math.min(max??Infinity,next));if(Number.isFinite(bounded))onChange(String(bounded));
      } else onKeyDown?.(event);
    }}/>;
}
