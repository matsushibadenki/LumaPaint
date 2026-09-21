import { useRef, type KeyboardEvent } from 'react';
import type { Locale } from '../i18n';
import { modeLabels, toolModes, type ToolMode } from '../tool-modes';
import { workspaceMessages } from '../workspace-i18n';
import { Icon } from './Icon';

const icons = { paint: 'brush', vector: 'vector', layout: 'layout', animation: 'animation' } as const;

export function ToolModeSwitch({ mode, locale, onChange }: {
  mode: ToolMode; locale: Locale; onChange: (mode: ToolMode) => void;
}) {
  const group = useRef<HTMLDivElement>(null);
  const t = workspaceMessages[locale];
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const offsets: Record<string, number> = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: 2, ArrowUp: -2 };
    if (!(event.key in offsets) && event.key !== 'Home' && event.key !== 'End') return;
    event.preventDefault();
    event.stopPropagation();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? toolModes.length - 1
      : (index + offsets[event.key] + toolModes.length) % toolModes.length;
    onChange(toolModes[next]);
    group.current?.querySelectorAll<HTMLButtonElement>('button')[next]?.focus();
  };
  return <div ref={group} className="tool-mode-switch" role="radiogroup" aria-label={t.switchToolMode}>
    {toolModes.map((item, index) => <button key={item} type="button" role="radio"
      className="tool-mode-choice" aria-checked={mode === item} tabIndex={mode === item ? 0 : -1}
      aria-label={t[modeLabels[item]]} title={t[modeLabels[item]]}
      onKeyDown={event => onKeyDown(event, index)} onClick={() => onChange(item)}>
      <Icon name={icons[item]} />
    </button>)}
  </div>;
}
