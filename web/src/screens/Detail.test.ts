import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { DETAIL } from '../app/copy.ts';
import type { Live } from '../app/operate.svelte.ts';
import { DESK_DEMO } from '../app/sample.ts';
import type { HelloResp } from '../serial/hello.ts';
import type { VersionResp } from '../wasm/dagon.ts';
import Detail from './Detail.svelte';

const V = (major: number, minor: number, patch: number) => (major << 24) | (minor << 16) | patch;
const PORT_SELF_BOOT = 1;
const PORT_VENDOR_STAGE2 = 2;

const hello = (portKind: number): HelloResp => ({
  protoVer: 1,
  role: 1,
  portKind,
  mcuId: 2,
  mode: 0,
  activeVersion: V(1, 0, 0),
  trial: false,
  keyId: new Uint8Array(8),
  maxPayload: 256,
});

const answered = (portKind: number, version: VersionResp | null | undefined): Live => ({
  responding: true,
  rttMs: 3,
  lastSeen: 0,
  hello: hello(portKind),
  status: null,
  version,
});

const detail = (live: Live) =>
  render(Detail, { props: { config: DESK_DEMO, live: { seg: live }, selected: 'node:seg', connected: true } }).body;

/** The value of row `key` under 詳しく見る. */
function row(body: string, key: string): string | null {
  const m = body.match(new RegExp(`<dt[^>]*>${key.replace(/[[\]]/g, '\\$&')}</dt>\\s*<dd[^>]*>([^<]*)</dd>`));
  return m ? m[1] : null;
}

const VERSION: VersionResp = {
  wire: 1,
  mcuId: 2,
  stage0: V(0, 1, 0),
  bootCore: V(1, 0, 0),
  coreSlots: { active: 0, slots: [{ state: 4, version: V(1, 0, 0) }, { state: 1, version: V(1, 1, 0) }] },
  appSlots: null,
};

describe('node detail: VERSION under 詳しく見る', () => {
  test('stage0 and each boot core slot with the running one', () => {
    const body = detail(answered(PORT_SELF_BOOT, VERSION));
    expect(row(body, 'stage0')).toBe('0.1.0');
    expect(row(body, 'boot_core[0]')).toBe('state 4 · 1.0.0 · running');
    expect(row(body, 'boot_core[1]')).toBe('state 1 · 1.1.0');
  });

  test('an agent that predates VERSION: every VERSION value is 不明', () => {
    const body = detail(answered(PORT_SELF_BOOT, null));
    for (const key of ['stage0', 'boot_core[0]', 'boot_core[1]']) expect(row(body, key), key).toBe(DETAIL.unknown);
  });

  test('a value the node left out is 不明 (a stage0 without its mark, unread boot core slots)', () => {
    const body = detail(answered(PORT_SELF_BOOT, { ...VERSION, stage0: null, bootCore: null, coreSlots: null }));
    for (const key of ['stage0', 'boot_core[0]', 'boot_core[1]']) expect(row(body, key), key).toBe(DETAIL.unknown);
  });

  test('an MCU booted by its vendor 2nd stage has no stage0 and no boot core: なし', () => {
    for (const v of [{ ...VERSION, stage0: null, bootCore: null, coreSlots: null }, null]) {
      const body = detail(answered(PORT_VENDOR_STAGE2, v));
      for (const key of ['stage0', 'boot_core[0]', 'boot_core[1]']) expect(row(body, key), key).toBe(DETAIL.none);
    }
  });

  test('not read yet: no row', () => {
    expect(row(detail(answered(PORT_SELF_BOOT, undefined)), 'stage0')).toBeNull();
  });
});
