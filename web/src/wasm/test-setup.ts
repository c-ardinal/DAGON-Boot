// Vitest runs in Node, where the generated loader cannot fetch the .wasm; load it from disk.
import { readFileSync } from 'node:fs';
import { initSync } from './dagon.ts';

initSync({ module: readFileSync(new URL('./pkg/dagon_wasm_bg.wasm', import.meta.url)) });
