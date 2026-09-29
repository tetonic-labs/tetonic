import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import '@fontsource-variable/inter';
import '@fontsource/plus-jakarta-sans/latin-800.css';
import '@fontsource/plus-jakarta-sans/latin-700.css';
import '@fontsource/jetbrains-mono/latin-400.css';
import '@fontsource/jetbrains-mono/latin-600.css';
import './index.css';
import './workspace.css';
import './canvas.css';
import './workroom.css';
import './team-room.css';
import './brand.css';
import './tools.css';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
