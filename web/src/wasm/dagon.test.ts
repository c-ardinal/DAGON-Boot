import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { decodeHelloResp, newHost, supportedBoards, topo_check, topo_compile, topo_targets, verify_release } from './dagon.ts';
import type { HostLink } from './pkg/dagon_wasm.js';

const repo = (rel: string) => new URL(`../../../${rel}`, import.meta.url);
const golden = (name: string) => new Uint8Array(readFileSync(repo(`crates/dagon-format/golden/${name}`)));
// RFC 8032 section 7.1 TEST 1 public key, which signs the golden release.
const TEST_PUBKEY = Uint8Array.from(
  'd75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a'.match(/../g)!,
  (h) => parseInt(h, 16),
);

describe('dagon-wasm exports', () => {
  test('supported boards come from the Rust table', () => {
    const mcus = supportedBoards().map((b) => b.mcu);
    expect(mcus).toContain('rp2040');
    expect(mcus).toContain('stm32f401re');
  });

  test('sample checks clean and compiles to the golden binary', () => {
    const yaml = new TextDecoder().decode(readFileSync(repo('topology/sample.yaml')));
    expect(JSON.parse(topo_check(yaml)).ok).toBe(true);
    expect(topo_compile(yaml)).toEqual(golden('topology_sample.bin'));
    expect(JSON.parse(topo_check('nodes: [')).ok).toBe(false);
    expect(() => topo_compile('nodes: [')).toThrow();
  });

  test('golden release verifies; a flipped byte does not', () => {
    const dgb = golden('release.dgb');
    expect(() => verify_release(dgb, TEST_PUBKEY)).not.toThrow();
    const bad = dgb.slice();
    bad[bad.length - 1] ^= 1;
    expect(() => verify_release(bad, TEST_PUBKEY)).toThrow();
  });

  test('a short HELLO body throws', () => {
    expect(() => decodeHelloResp(new Uint8Array(23))).toThrow();
  });
});

describe("each node's target is the one dagon-host gives", () => {
  const sample = new TextDecoder().decode(readFileSync(repo('topology/sample.yaml')));
  const idle: HostLink = {
    write: async () => undefined,
    read: async () => new Uint8Array(),
    reopen: async () => undefined,
    maxPayload: () => 1024,
    reconnectKind: () => 'reopen',
    now: () => 0,
    sleep: async () => undefined,
  };

  test('the connected node is local, the others linked over their own entries, the same for the host', () => {
    const t = topo_targets(sample);
    expect(t.hub).toEqual({ kind: 'local' });
    const [seg, watch] = [t.seg, t.watch];
    expect(seg.kind === 'linked' && watch.kind === 'linked' && seg.link !== watch.link).toBe(true);
    expect(seg.kind === 'linked' && seg.addr).toBe(0x42);
    const h = newHost(idle as never);
    expect(h.setTimings(sample)).toEqual(t);
    h.free();
  });

  test("the order of a link's ends changes nothing", () => {
    const reversed = sample.replace(/( {6}- \{ node: hub, port: i2c0[^\n]*\n)( {6}- \{ node: seg[^\n]*\n)/, '$2$1');
    expect(reversed).not.toBe(sample);
    expect(topo_targets(reversed)).toEqual(topo_targets(sample));
  });

  test('a node on a port the connected node relays over no entry of has no target', () => {
    // The watch on the hub's USB port, which the hub relays over no entry of.
    const onUsb = sample.replace(
      /  - kind: uart\n    iface: pio-uart\n    ends:\n.*\n.*\n    baud: 115200\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n/,
      '  - kind: usb-cdc\n    ends:\n      - { node: hub, port: usb }\n      - { node: watch, port: usb-serial-jtag }\n',
    ).replace('  watch: [local, redistribute, rom-coop]', '  watch: [local]');
    expect(onUsb).not.toBe(sample);
    expect(JSON.parse(topo_check(onUsb)).ok).toBe(true);
    const t = topo_targets(onUsb);
    expect(t.hub).toEqual({ kind: 'local' });
    expect(t.watch).toBeUndefined();
    expect(t.seg).toEqual(topo_targets(sample).seg);
  });
});
