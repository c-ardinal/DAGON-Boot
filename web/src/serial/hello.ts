// HELLO payload shapes (dagon_proto::payload::{HelloReq, HelloResp}). The page
// encodes requests and decodes responses through dagon-wasm; the response
// encoder here exists only for the mock hub, and writes through dagon-proto's
// field writer too.
import { writePayload } from '../wasm/dagon.ts';
import { CMD } from './frame.ts';

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

function le(value: number, bytes: number): Uint8Array {
  const out = new Uint8Array(bytes);
  for (let i = 0; i < bytes; i++) out[i] = (value >>> (8 * i)) & 0xff;
  return out;
}

/** Core: proto_ver u32, max_payload u16; fields 1 to 7 as dagon_proto::table::hello numbers them. */
export function encodeHelloResp(r: HelloResp): Uint8Array {
  return writePayload(CMD.HELLO, true, {
    core: Uint8Array.of(...le(r.protoVer, 4), ...le(r.maxPayload, 2)),
    fields: [
      [1, Uint8Array.of(r.role)],
      [2, Uint8Array.of(r.portKind)],
      [3, le(r.mcuId, 2)],
      [4, Uint8Array.of(r.mode)],
      [5, le(r.activeVersion, 4)],
      [6, Uint8Array.of(r.trial ? 1 : 0)],
      [7, r.keyId.subarray(0, 8)],
    ],
  });
}

export function versionRaw(major: number, minor: number, patch: number): number {
  return ((major << 24) | (minor << 16) | patch) >>> 0;
}
