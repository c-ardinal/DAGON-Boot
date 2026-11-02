import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { CMD, FLAG_RESP, FrameDecoder, crc32, encodeFrame, parseFrame } from './frame.ts';
import { decodeHelloReq, decodeHelloResp, encodeHelloReq, encodeHelloResp, versionRaw, versionText } from './hello.ts';

const golden = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(new URL(`../../../crates/dagon-format/golden/${name}`, import.meta.url)));

describe('frame codec against the Rust golden frames', () => {
  test('crc32 check value', () => {
    expect(crc32(new TextEncoder().encode('123456789'))).toBe(0xcbf43926);
  });

  test('HELLO request is byte-identical', () => {
    const file = golden('frame_hello_req.bin');
    const built = encodeFrame({ flags: 0, seq: 1, cmd: CMD.HELLO, payload: encodeHelloReq({ protoVer: 1, maxPayload: 1088 }) });
    expect(built).toEqual(file);
    expect(decodeHelloReq(parseFrame(file).payload)).toEqual({ protoVer: 1, maxPayload: 1088 });
  });

  test('HELLO response decodes and re-encodes identically', () => {
    const file = golden('frame_hello_resp.bin');
    const [raw] = new FrameDecoder().push(file);
    const f = parseFrame(raw);
    expect(f.flags & FLAG_RESP).toBe(FLAG_RESP);
    expect(f.payload[0]).toBe(0);
    const h = decodeHelloResp(f.payload.subarray(1));
    expect(h).toMatchObject({ protoVer: 1, role: 1, portKind: 1, mcuId: 2, mode: 0, trial: true, maxPayload: 256 });
    expect(versionText(h.activeVersion)).toBe('1.1.0');
    expect(Array.from(h.keyId)).toEqual([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
    const body = encodeHelloResp(h);
    const again = encodeFrame({ flags: f.flags, seq: f.seq, cmd: f.cmd, payload: Uint8Array.of(0, ...body) });
    expect(again).toEqual(file);
  });

  test('every golden frame passes the stream decoder', () => {
    for (const name of ['frame_relay_hello.bin', 'frame_status_resp.bin', 'frame_xfer_hdr.bin', 'frame_show.bin']) {
      const file = golden(name);
      expect(new FrameDecoder().push(file)).toEqual([file]);
    }
  });
});

describe('stream decoder', () => {
  const a = encodeFrame({ flags: 0, seq: 7, cmd: CMD.HELLO, payload: Uint8Array.of(1, 2, 3) });

  test('skips noise, waits for split frames, drops bad CRC', () => {
    const d = new FrameDecoder();
    const noise = new TextEncoder().encode('boot log\xda');
    const bad = a.slice();
    bad[9] ^= 1;
    expect(d.push(noise)).toEqual([]);
    expect(d.push(bad)).toEqual([]);
    expect(d.push(a.subarray(0, 5))).toEqual([]);
    expect(d.push(a.subarray(5))).toEqual([a]);
  });

  test('two frames in one chunk', () => {
    expect(new FrameDecoder().push(Uint8Array.of(...a, ...a))).toEqual([a, a]);
  });

  test('version u32 layout', () => {
    expect(versionRaw(1, 1, 0)).toBe(0x01010000);
    expect(versionText(versionRaw(1, 10, 300))).toBe('1.10.300');
  });
});
