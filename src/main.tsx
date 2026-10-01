import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Workspace } from './Workspace';
import './workspace.css';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { ModalPage } from './ModalPage';

createRoot(document.getElementById('root')!).render(<StrictMode>{isTauri() && getCurrentWebviewWindow().label.startsWith('modal-') ? <ModalPage /> : <Workspace />}</StrictMode>);
