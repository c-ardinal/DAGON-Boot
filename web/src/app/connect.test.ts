import { describe, expect, test } from 'vitest';
import { MockTransport, SAMPLE_HUB_HELLO } from '../serial/mock.ts';
import { ConnectError, failureOf, hello } from './connect.ts';

describe('HELLO over the mock transport', () => {
  test('returns the hub HELLO', async () => {
    const t = new MockTransport();
    await t.open();
    expect(await hello(t)).toEqual(SAMPLE_HUB_HELLO);
  });

  test('a silent device ends in no-response', async () => {
    const t = new MockTransport({ silent: true });
    await t.open();
    await expect(hello(t)).rejects.toEqual(new ConnectError('no-response'));
  });

  test('writing before open fails', async () => {
    await expect(new MockTransport().write(new Uint8Array(12))).rejects.toThrow();
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
