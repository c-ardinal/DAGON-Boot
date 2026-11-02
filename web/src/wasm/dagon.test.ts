import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { decodeHelloResp, supported_boards, topo_check, topo_compile, verify_release } from './dagon.ts';

const repo = (rel: string) => new URL(`../../../${rel}`, import.meta.url);
const golden = (name: string) => new Uint8Array(readFileSync(repo(`crates/dagon-format/golden/${name}`)));
// RFC 8032 section 7.1 TEST 1 public key, which signs the golden release.
const TEST_PUBKEY = Uint8Array.from(
  'd75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a'.match(/../g)!,
  (h) => parseInt(h, 16),
);

describe('dagon-wasm exports', () => {
  test('supported boards come from the Rust table', () => {
    const mcus = supported_boards().map((b) => b.mcu);
    expect(mcus).toContain('rp2040');
    expect(mcus).toContain('stm32f401re');
  });

  test('desk demo checks clean and compiles to the golden binary', () => {
    const yaml = new TextDecoder().decode(readFileSync(repo('topology/desk-demo.yaml')));
    expect(JSON.parse(topo_check(yaml)).ok).toBe(true);
    expect(topo_compile(yaml)).toEqual(golden('topology_desk_demo.bin'));
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
