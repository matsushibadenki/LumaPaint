import {createRoot} from 'react-dom/client';
import {SettingsDialog} from './components/SettingsDialog';
import './workspace.css';
import type {Locale} from './i18n';
document.documentElement.dataset.theme='dark';
createRoot(document.getElementById('root')!).render(<SettingsDialog locale={(window as unknown as {qaLocale:Locale}).qaLocale} theme="dark" onLocale={()=>{}} onTheme={()=>{}} onClose={()=>{}}/>);
