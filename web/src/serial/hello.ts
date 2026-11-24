// HELLO payload shapes (dagon_proto::payload::{HelloReq, HelloResp}). The page
// encodes requests and decodes responses through dagon-wasm; the response
// encoder here exists only for the mock PC-connected node, and is dagon-proto's too.
import { respond } from '../wasm/dagon.ts';

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
  /** The commands the node serves, as dagon-proto packs them; left out by a node that does not say. */
  cmds?: Uint8Array;
}

/** The body of an OK HELLO response, written by dagon-proto. */
export function encodeHelloResp(r: HelloResp): Uint8Array {
  return respond.helloBody(r);
}

export function versionRaw(major: number, minor: number, patch: number): number {
  return ((major << 24) | (minor << 16) | patch) >>> 0;
}
