import { messages, type Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { IconToolMenu } from './IconToolMenu';

export type MagnificationTool = 'zoomIn' | 'zoomOut';
export type ZoomTool = MagnificationTool | 'hand';
export function ZoomToolMenu({ locale, selected, active, enabled, onSelect, onError, onSettings }: {
  onSettings?: (tool: MagnificationTool) => void;
  locale: Locale; selected: MagnificationTool; active: boolean; enabled: boolean; onSelect: (tool: MagnificationTool) => void; onError: (message: string) => void;
}) {
  const t = messages[locale];
  return <IconToolMenu label={workspaceMessages[locale].zoomTools} selected={selected} active={active} enabled={enabled}
    choices={[{ id: 'zoomIn', icon: 'zoomIn', label: t.zoomIn }, { id: 'zoomOut', icon: 'zoomOut', label: t.zoomOut }]}
    onSelect={tool => onSelect(tool as MagnificationTool)} onError={onError} onSettings={tool=>onSettings?.(tool as MagnificationTool)} />;
}
