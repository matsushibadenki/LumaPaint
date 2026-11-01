import type { KeyboardEvent } from 'react';

/** Parse only complete, finite decimal values. In-progress text remains editable. */
export function parseSignedNumber(text:string):number|null {
  if (!/^-?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(text)) return null;
  const value=Number(text);
  return Number.isFinite(value)?value:null;
}

export function SignedNumberInput({id,className,label,value,onChange,onBlur,onFocus,onKeyDown,disabled,min,max,placeholder,allowEmpty=false,stepBase=0,step=1,stepOrigin}:{
  id?:string;className?:string;label:string;value:string;onChange:(value:string)=>void;onBlur?:()=>void;onFocus?:()=>void;onKeyDown?:(event:KeyboardEvent<HTMLInputElement>)=>void;
  disabled?:boolean;min?:number;max?:number;placeholder?:string;allowEmpty?:boolean;stepBase?:number;step?:number;stepOrigin?:number;
}) {
  const number=parseSignedNumber(value);
  const valid=number!==null&&(min===undefined||number>=min)&&(max===undefined||number<=max);
  return <input id={id} className={className} type="text" inputMode="decimal" role="spinbutton" aria-label={label} aria-valuemin={min} aria-valuemax={max}
    aria-valuenow={valid?number:undefined} aria-invalid={!valid&&!(allowEmpty&&value==='')} value={value} disabled={disabled} placeholder={placeholder}
    onChange={event=>onChange(event.currentTarget.value)} onBlur={onBlur} onFocus={onFocus} onKeyDown={event=>{
      if(event.nativeEvent.isComposing)return;
      if(event.key==='ArrowUp'||event.key==='ArrowDown'){
        // Rounded displays may step from an untouched model value instead.
        event.preventDefault();const next=(stepOrigin??(valid?number:stepBase))+(event.key==='ArrowUp'?step:-step);
        const bounded=Math.max(min??-Infinity,Math.min(max??Infinity,next));if(Number.isFinite(bounded))onChange(String(step === 1 ? bounded : Number(bounded.toPrecision(12))));
      } else onKeyDown?.(event);
    }}/>;
}
