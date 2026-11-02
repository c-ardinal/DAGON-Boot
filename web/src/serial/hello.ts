// HELLO payloads, same layout as dagon_proto::payload::{HelloReq, HelloResp}.
// ponytail: hand-written until dagon-wasm exposes the Rust codec; then the
// page calls that and this file shrinks to the mock's encoder.

export const HELLO_RESP_LEN = 24;
export const ROLE_HUB = 0;
export const ROLE_NODE = 1;

export interface HelloReq {
  protoVer: number;
  maxPayload: number;
}

export interface HelloResp {
  protoVer: number;
  role: number;
  portKind: number;
  mcuId: number;
  mode: number;
  /** Raw u32: major << 24 | minor << 16 | patch. */
  activeVersion: number;
  trial: boolean;
  keyId: Uint8Array;
  maxPayload: number;
}

export function encodeHelloReq(r: HelloReq): Uint8Array {
  const out = new Uint8Array(6);
  const dv = new DataView(out.buffer);
  dv.setUint32(0, r.protoVer, true);
  dv.setUint16(4, r.maxPayload, true);
  return out;
}

export function decodeHelloReq(b: Uint8Array): HelloReq {
  if (b.length !== 6) throw new RangeError('HELLO request length');
  const dv = new DataView(b.buffer, b.byteOffset, b.length);
  return { protoVer: dv.getUint32(0, true), maxPayload: dv.getUint16(4, true) };
}

export function encodeHelloResp(r: HelloResp): Uint8Array {
  const out = new Uint8Array(HELLO_RESP_LEN);
  const dv = new DataView(out.buffer);
  dv.setUint32(0, r.protoVer, true);
  out[4] = r.role;
  out[5] = r.portKind;
  dv.setUint16(6, r.mcuId, true);
  out[8] = r.mode;
  dv.setUint32(9, r.activeVersion, true);
  out[13] = r.trial ? 1 : 0;
  out.set(r.keyId.subarray(0, 8), 14);
  dv.setUint16(22, r.maxPayload, true);
  return out;
}

/** Body after the status byte. Throws on a wrong length or trial value. */
export function decodeHelloResp(b: Uint8Array): HelloResp {
  if (b.length !== HELLO_RESP_LEN) throw new RangeError('HELLO response length');
  const dv = new DataView(b.buffer, b.byteOffset, b.length);
  if (b[13] > 1) throw new RangeError('HELLO trial flag');
  return {
    protoVer: dv.getUint32(0, true),
    role: b[4],
    portKind: b[5],
    mcuId: dv.getUint16(6, true),
    mode: b[8],
    activeVersion: dv.getUint32(9, true),
    trial: b[13] === 1,
    keyId: b.slice(14, 22),
    maxPayload: dv.getUint16(22, true),
  };
}

// ponytail: display-only split of the version u32; replace with the
// dagon-format formatter from dagon-wasm when it lands.
export function versionText(raw: number): string {
  return `${raw >>> 24}.${(raw >>> 16) & 0xff}.${raw & 0xffff}`;
}

export function versionRaw(major: number, minor: number, patch: number): number {
  return ((major << 24) | (minor << 16) | patch) >>> 0;
}
