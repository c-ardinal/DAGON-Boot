// DAGON link v1 frame, byte-for-byte the same as crates/dagon-proto:
// SOF DA 6F | ver 01 | flags | seq | cmd | len u16 LE | payload | crc32 LE.
// The CRC (IEEE, reflected) covers ver..payload.

export const SOF0 = 0xda;
export const SOF1 = 0x6f;
export const PROTO_VERSION = 0x01;
export const HEADER_LEN = 8;
export const CRC_LEN = 4;
export const MAX_PAYLOAD_USB = 1088;

export const FLAG_RESP = 1 << 0;

export const CMD = {
  HELLO: 0x01,
  STATUS: 0x02,
} as const;

export const STATUS_OK = 0x00;

export interface Frame {
  flags: number;
  seq: number;
  cmd: number;
  payload: Uint8Array;
}

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[i] = c >>> 0;
  }
  return t;
})();

export function crc32(data: Uint8Array): number {
  let crc = 0xffffffff;
  for (const b of data) crc = CRC_TABLE[(crc ^ b) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

export function encodeFrame(f: Frame): Uint8Array {
  const n = f.payload.length;
  const out = new Uint8Array(HEADER_LEN + n + CRC_LEN);
  const dv = new DataView(out.buffer);
  out.set([SOF0, SOF1, PROTO_VERSION, f.flags, f.seq, f.cmd]);
  dv.setUint16(6, n, true);
  out.set(f.payload, HEADER_LEN);
  dv.setUint32(HEADER_LEN + n, crc32(out.subarray(2, HEADER_LEN + n)), true);
  return out;
}

/** Splits a frame that already passed {@link FrameDecoder} or came from {@link encodeFrame}. */
export function parseFrame(raw: Uint8Array): Frame {
  return { flags: raw[3], seq: raw[4], cmd: raw[5], payload: raw.slice(HEADER_LEN, raw.length - CRC_LEN) };
}

/**
 * Cuts complete frames out of a byte stream. Bytes before a SOF and frames
 * with a bad version, length or CRC are skipped, like the Rust side does
 * with `find_sof` after a decode error.
 */
export class FrameDecoder {
  private buf = new Uint8Array(0);

  constructor(private readonly maxPayload = MAX_PAYLOAD_USB) {}

  /** Returns each complete, CRC-checked frame as raw bytes. */
  push(chunk: Uint8Array): Uint8Array[] {
    const joined = new Uint8Array(this.buf.length + chunk.length);
    joined.set(this.buf);
    joined.set(chunk, this.buf.length);
    const frames: Uint8Array[] = [];
    let at = 0;
    for (;;) {
      const sof = joined.indexOf(SOF0, at);
      if (sof < 0) {
        at = joined.length;
        break;
      }
      const r = this.frameLen(joined, sof);
      if (r === 'incomplete') {
        at = sof;
        break;
      }
      if (r === 'bad') {
        at = sof + 1;
        continue;
      }
      frames.push(joined.slice(sof, sof + r));
      at = sof + r;
    }
    this.buf = joined.slice(at);
    return frames;
  }

  private frameLen(b: Uint8Array, s: number): 'incomplete' | 'bad' | number {
    const avail = b.length - s;
    if (avail >= 2 && b[s + 1] !== SOF1) return 'bad';
    if (avail < HEADER_LEN) return 'incomplete';
    if (b[s + 2] !== PROTO_VERSION) return 'bad';
    const len = b[s + 6] | (b[s + 7] << 8);
    if (len > this.maxPayload) return 'bad';
    const total = HEADER_LEN + len + CRC_LEN;
    if (avail < total) return 'incomplete';
    const end = s + HEADER_LEN + len;
    const crc = new DataView(b.buffer, b.byteOffset).getUint32(end, true);
    return crc32(b.subarray(s + 2, end)) === crc ? total : 'bad';
  }
}
