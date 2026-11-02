import { mount } from 'svelte';
import './styles/tokens.css';
import './styles/base.css';
import App from './App.svelte';
import { initDagon } from './wasm/dagon.ts';

void initDagon().then(() => mount(App, { target: document.getElementById('app')! }));
