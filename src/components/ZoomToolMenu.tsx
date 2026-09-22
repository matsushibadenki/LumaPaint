import { messages, type Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { IconToolMenu } from './IconToolMenu';

export type ZoomTool = 'zoomIn' | 'zoomOut' | 'hand';
export function ZoomToolMenu({ locale, selected, active, enabled, onSelect, onError }: {
  locale: Locale; selected: ZoomTool; active: boolean; enabled: boolean; onSelect: (tool: ZoomTool) => void; onError: (message: string) => void;
}) {
  const t = messages[locale];
  return <IconToolMenu label={workspaceMessages[locale].zoomTools} selected={selected} active={active} enabled={enabled}
    choices={[{ id: 'zoomIn', icon: 'zoomIn', label: t.zoomIn }, { id: 'zoomOut', icon: 'zoomOut', label: t.zoomOut }, { id: 'hand', icon: 'hand', label: t.hand }]}
    onSelect={tool => onSelect(tool as ZoomTool)} onError={onError} />;
}
