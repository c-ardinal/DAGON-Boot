// HELLO payload shapes (dagon_proto::payload::{HelloReq, HelloResp}). The page
// encodes requests and decodes responses through dagon-wasm; the response
// encoder here exists only for the mock hub.

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

export function versionRaw(major: number, minor: number, patch: number): number {
  return ((major << 24) | (minor << 16) | patch) >>> 0;
}
