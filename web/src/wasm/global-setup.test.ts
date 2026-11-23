import { describe, expect, test } from 'vitest';
import { wasmProblem } from './global-setup.ts';

describe('the WebAssembly the tests run on', () => {
  test('the development build (with the simulator) runs them', () => {
    expect(wasmProblem('export class FrameCutter {}\nexport class Simulator {\n}')).toBeNull();
  });
  test('a release build, or none, is named before any test runs', () => {
    expect(wasmProblem('export class FrameCutter {}\n// Simulator is not built\n')).toMatch(/リリース用/);
    expect(wasmProblem(null)).toMatch(/ありません/);
  });
});
