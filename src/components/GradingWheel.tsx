import { useRef, type PointerEvent } from 'react';

/** Local gesture draft only. The document receives one command on release. */
export function GradingWheel({ label, hue, saturation, disabled, onChange, onCommit }: {
  label: string; hue: number; saturation: number; disabled: boolean;
  onChange: (hue: number, saturation: number) => void;
  onCommit: (hue: number, saturation: number) => void;
}) {
  const gesture = useRef<{ id: number; hue: number; saturation: number } | null>(null);
  const position = (e: PointerEvent<HTMLButtonElement>): [number, number] => {
    const rect = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - rect.left - rect.width / 2;
    const y = e.clientY - rect.top - rect.height / 2;
    return [Math.hypot(x,y) < 1 ? hue : (Math.atan2(y,x) * 180 / Math.PI + 360) % 360,
      Math.min(100, Math.hypot(x,y) / (Math.min(rect.width,rect.height)/2) * 100)];
  };
  const angle = hue * Math.PI / 180;
  return <button type="button" className="effect-grading-wheel" aria-label={label} title={label} disabled={disabled}
    onPointerDown={e=>{ if(e.button!==0)return; e.preventDefault(); e.currentTarget.focus(); gesture.current={id:e.pointerId,hue,saturation};e.currentTarget.setPointerCapture(e.pointerId);onChange(...position(e)); }}
    onPointerMove={e=>{if(gesture.current?.id===e.pointerId)onChange(...position(e));}}
    onPointerUp={e=>{if(gesture.current?.id!==e.pointerId)return; const next=position(e);gesture.current=null;onCommit(...next);}}
    onPointerCancel={()=>{const start=gesture.current;gesture.current=null;if(start)onChange(start.hue,start.saturation);}}
    onLostPointerCapture={()=>{const start=gesture.current;gesture.current=null;if(start)onChange(start.hue,start.saturation);}}
    onKeyDown={e=>{const key=e.key;if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home'].includes(key))return;e.preventDefault();onCommit((hue+(key==='ArrowLeft'?-1:key==='ArrowRight'?1:0)+360)%360,key==='Home'?0:Math.max(0,Math.min(100,saturation+(key==='ArrowUp'?1:key==='ArrowDown'?-1:0))));}}>
    <span className="effect-wheel-marker" style={{left:`${50+Math.cos(angle)*saturation/2}%`,top:`${50+Math.sin(angle)*saturation/2}%`}}/>
  </button>;
}
