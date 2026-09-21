import { useState, type Dispatch, type SetStateAction } from 'react';
import { messages, type Locale } from '../i18n';
import { workspaceMessages } from '../workspace-i18n';
import { IconToolMenu } from './IconToolMenu';

export function ZoomToolMenu({ locale, zoom, enabled, onZoom, onError }: {
  locale: Locale; zoom: number; enabled: boolean; onZoom: Dispatch<SetStateAction<number>>; onError: (message: string) => void;
}) {
  const [selected, setSelected] = useState('zoomIn');
  const t = messages[locale];
  return <IconToolMenu label={workspaceMessages[locale].zoomTools} selected={selected} active={false} enabled={enabled}
    choices={[{ id: 'zoomIn', icon: 'zoomIn', label: t.zoomIn, enabled: zoom < 4 }, { id: 'zoomOut', icon: 'zoomOut', label: t.zoomOut, enabled: zoom > 0.25 }]}
    onSelect={action => { setSelected(action); onZoom(current => Math.max(0.25, Math.min(4, action === 'zoomIn' ? current * 1.25 : current / 1.25))); }} onError={onError} />;
}
