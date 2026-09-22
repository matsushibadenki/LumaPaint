import type { CanvasTool } from '../bridge';
import type { Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { IconToolMenu } from './IconToolMenu';

export type VectorShapeTool = Extract<CanvasTool, 'vectorRectangle' | 'vectorEllipse'>;

export function VectorShapeToolMenu({ locale, selected, active, enabled, onSelect, onError }: {
  locale: Locale; selected: VectorShapeTool; active: boolean; enabled: boolean;
  onSelect: (tool: VectorShapeTool) => void; onError: (message: string) => void;
}) {
  const t = workspaceMessages[locale];
  return <IconToolMenu label={t.vectorShapes} selected={selected} active={active} enabled={enabled}
    choices={[{ id: 'vectorRectangle', label: t.vectorRectangle, icon: 'vectorRectangle', shortcut: 'U' }, { id: 'vectorEllipse', label: t.vectorEllipse, icon: 'vectorEllipse', shortcut: 'Shift+U' }]}
    onSelect={tool => onSelect(tool as VectorShapeTool)} onError={onError} />;
}
