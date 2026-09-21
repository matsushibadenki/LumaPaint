import type { Locale } from '../i18n';
import type { CanvasTool } from '../bridge';
import { workspaceMessages } from '../workspace-i18n';
import { IconToolMenu } from './IconToolMenu';

export type SelectionTool = Extract<CanvasTool, 'rectangle' | 'ellipse'>;
export function SelectionToolMenu({ locale, selected, active, enabled, onSelect, onError }: {
  locale: Locale; selected: SelectionTool; active: boolean; enabled: boolean;
  onSelect: (tool: SelectionTool) => void; onError: (message: string) => void;
}) {
  const t = workspaceMessages[locale];
  return <IconToolMenu label={t.selectionTools} selected={selected} active={active} enabled={enabled} selectionShortcuts
    choices={[{ id: 'rectangle', label: t.rectangle, icon: 'rectangle', shortcut: 'M' }, { id: 'ellipse', label: t.ellipse, icon: 'ellipse', shortcut: 'Shift+M' }]}
    onSelect={tool => onSelect(tool as SelectionTool)} onError={onError} />;
}
