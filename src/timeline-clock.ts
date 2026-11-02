// Transport packets contain only Rust-owned playback state. Sequence numbers
// prevent a delayed command reply from rewinding a newer native notification.
export type TimelineClock={sequence:number;documentId:number;revision:number;frame:number;playing:boolean;preview:boolean};
type ClockView={frame:number;playing:boolean;preview:boolean;data:{documentId:number;revision:number;duration:number}|null;clock?:TimelineClock|null};
export function applyTimelineClock<T extends ClockView>(state:T,clock:TimelineClock):T {
 if(!state.data||clock.documentId!==state.data.documentId||clock.revision!==state.data.revision||!Number.isSafeInteger(clock.sequence)||clock.sequence<=0||!Number.isInteger(clock.frame)||clock.frame<0||clock.frame>=state.data.duration||typeof clock.playing!=='boolean'||typeof clock.preview!=='boolean'||clock.sequence<=(state.clock?.sequence??0))return state;
 return {...state,clock,frame:clock.frame,playing:clock.playing,preview:clock.preview};
}
export function mergeTimelineClock<T extends ClockView>(previous:T,next:T):T {
 if(!next.clock||!previous.clock||previous.clock.sequence<=next.clock.sequence||previous.data?.documentId!==next.data?.documentId)return next;
 if(previous.data!.revision>next.data!.revision)return previous;
 if(previous.data!.revision!==next.data!.revision)return next;
 return {...next,clock:previous.clock,frame:previous.frame,playing:previous.playing,preview:previous.preview};
}
