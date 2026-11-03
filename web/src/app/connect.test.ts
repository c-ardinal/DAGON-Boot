import { describe, expect, test } from 'vitest';
import { MockTransport } from '../serial/mock.ts';
import { failureOf, wasmReady, webSerial } from './connect.ts';

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

describe('dagon demo never opens Web Serial', () => {
  function fakeSerial() {
    const listeners: Record<string, ((e: Event) => void)[]> = {};
    let chooser = 0;
    const serial = {
      addEventListener: (type: string, f: (e: Event) => void) => (listeners[type] ??= []).push(f),
      requestPort: () => {
        chooser++;
        return Promise.reject(new DOMException('x', 'NotFoundError'));
      },
    } as unknown as Serial;
    const fire = (type: string) => listeners[type]?.forEach((f) => f(new Event(type)));
    return { serial, fire, chooser: () => chooser };
  }

  test('while the page drives the simulated desk, a plugged device and the chooser reach no port', async () => {
    const f = fakeSerial();
    const ws = webSerial(f.serial, () => true);
    const seen: string[] = [];
    ws.on('connect', (e) => seen.push(e.type));
    ws.on('disconnect', (e) => seen.push(e.type));
    f.fire('connect');
    f.fire('disconnect');
    expect(seen).toEqual([]);
    await expect(ws.chooseAndOpen()).rejects.toThrow();
    expect(f.chooser()).toBe(0);
  });

  test('with a real hub, Web Serial works as before', async () => {
    const f = fakeSerial();
    const ws = webSerial(f.serial, () => false);
    const seen: string[] = [];
    ws.on('connect', (e) => seen.push(e.type));
    f.fire('connect');
    expect(seen).toEqual(['connect']);
    await expect(ws.chooseAndOpen()).rejects.toThrow();
    expect(f.chooser()).toBe(1);
  });
});
