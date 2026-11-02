import { describe, expect, test } from 'vitest';
import { MockTransport } from '../serial/mock.ts';
import { failureOf, wasmReady } from './connect.ts';

describe('mock transport', () => {
  test('writing before open fails', async () => {
    await expect(new MockTransport().write(new Uint8Array(12))).rejects.toThrow();
  });
});

describe('dagon-wasm loading', () => {
  test('a failed load reports false instead of throwing', async () => {
    expect(await wasmReady(() => Promise.reject(new Error('404')))).toBe(false);
    expect(await wasmReady()).toBe(true);
  });
});

describe('Web Serial rejections', () => {
  test.each([
    ['NotFoundError', 'choose', 'not-found'],
    ['SecurityError', 'choose', 'denied'],
    ['SecurityError', 'open', 'denied'],
    ['NetworkError', 'open', 'busy'],
    ['InvalidStateError', 'open', 'busy'],
  ] as const)('%s while %s → %s', (name, phase, want) => {
    expect(failureOf(new DOMException('x', name), phase)).toBe(want);
  });
});
