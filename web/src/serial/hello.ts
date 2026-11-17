// HELLO payload shapes (dagon_proto::payload::{HelloReq, HelloResp}). The page
// encodes requests and decodes responses through dagon-wasm; the response
// encoder here exists only for the mock hub, and is dagon-proto's too.
import { respond } from '../wasm/dagon.ts';

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

/** The body of an OK HELLO response, written by dagon-proto. */
export function encodeHelloResp(r: HelloResp): Uint8Array {
  return respond.helloBody(r);
}

export function versionRaw(major: number, minor: number, patch: number): number {
  return ((major << 24) | (minor << 16) | patch) >>> 0;
}
