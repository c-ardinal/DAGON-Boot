// Before any test file: the tests need the development build of the WebAssembly
// (`npm run wasm:dev`: the simulator and the public test key). `npm test` builds it
// first; a bare `npx vitest` runs on whatever src/wasm/pkg holds, and a release
// build there (`npm run wasm`, `npm run build`) would fail dozens of tests for no
// reason of their own. This fails once, saying what to run instead.
import { existsSync, readFileSync } from 'node:fs';

const LOADER = new URL('./pkg/dagon_wasm.js', import.meta.url);

/** Why the built WebAssembly cannot run the tests, or null when it can. */
export function wasmProblem(loader: string | null): string | null {
  if (loader === null) return 'src/wasm/pkg にWebAssemblyがありません。';
  // Only the `sim` feature (wasm:dev) exports the simulator.
  if (!/^export class Simulator\b/m.test(loader)) return 'src/wasm/pkg のWebAssemblyはリリース用のビルド（シミュレータなし）です。';
  return null;
}

export default function setup(): void {
  const problem = wasmProblem(existsSync(LOADER) ? new TextDecoder().decode(readFileSync(LOADER)) : null);
  if (!problem) return;
  const e = new Error(`${problem} 試験用のビルドを「npm run wasm:dev」で作成するか、「npm test」で試験を実行してください。`);
  // The message alone: Vitest cannot map a global setup's stack back to this file and would fail on it instead.
  e.stack = e.message;
  throw e;
}
