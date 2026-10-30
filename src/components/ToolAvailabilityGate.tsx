import { useId, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

// Disabled controls do not reliably show native title tooltips in WKWebView.
export function ToolAvailabilityGate({ reason, children }: { reason?: string; children: ReactNode }) {
  const [hovered, setHovered] = useState(false);
  const id = useId();
  if (children == null) return null;
  const showReason = hovered && !!reason;
  return <span className="tool-availability" title={reason} data-unavailable={reason ? true : undefined}
    aria-describedby={showReason ? id : undefined}
    onPointerEnter={() => setHovered(true)} onPointerLeave={() => setHovered(false)}
    onClickCapture={event => { if (reason) { event.preventDefault(); event.stopPropagation(); } }}
    onDoubleClickCapture={event => { if (reason) { event.preventDefault(); event.stopPropagation(); } }}>
    <fieldset disabled={!!reason}>{children}</fieldset>
    {showReason && createPortal(<span id={id} role="tooltip" className="tool-unavailable-tooltip">{reason}</span>, document.body)}
  </span>;
}
