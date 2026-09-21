import { Icon } from './Icon';

export type ToolMode = 'paint' | 'vector';

export function ToolModeSwitch({ mode, paintLabel, vectorLabel, switchLabel, onChange }: {
  mode: ToolMode;
  paintLabel: string;
  vectorLabel: string;
  switchLabel: string;
  onChange: (mode: ToolMode) => void;
}) {
  const next = mode === 'paint' ? 'vector' : 'paint';
  return <button type="button" role="switch" aria-checked={mode === 'vector'}
    className="tool-mode-switch" data-mode={mode}
    aria-label={`${switchLabel}: ${mode === 'paint' ? paintLabel : vectorLabel}`}
    title={`${switchLabel}: ${mode === 'paint' ? paintLabel : vectorLabel}`}
    onClick={() => onChange(next)}>
    <span className="tool-mode-choice paint" aria-hidden="true"><Icon name="brush" /></span>
    <span className="tool-mode-choice vector" aria-hidden="true"><Icon name="vector" /></span>
  </button>;
}
