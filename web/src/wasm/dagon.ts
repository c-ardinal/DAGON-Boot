// The page's single entry to dagon-wasm (built into ./pkg by `npm run wasm`).
// Converts the generated classes into plain objects so nothing outside this
// file has to free WASM memory.
import init, { hello_decode, hello_request, supported_boards as boards, version_text } from './pkg/dagon_wasm.js';
import type { HelloReq, HelloResp } from '../serial/hello.ts';

export { initSync, topo_check, topo_compile, verify_release } from './pkg/dagon_wasm.js';

let loading: Promise<unknown> | undefined;

/**
 * Starts loading the .wasm next to the bundle on the first call and returns
 * the same promise afterwards. Await it before calling anything else here;
 * it rejects when the file cannot be fetched or instantiated.
 */
export function dagonReady(): Promise<unknown> {
  return (loading ??= init());
}

export interface Board {
  id: string;
  product: string;
  mcuName: string;
  mcu: string;
  flashBytes: number;
  i2cTarget: boolean;
  drawing: string;
}

export function supportedBoards(): Board[] {
  return boards().map((b) => {
    try {
      return {
        id: b.id,
        product: b.product,
        mcuName: b.mcu_name,
        mcu: b.mcu,
        flashBytes: b.flash_bytes,
        i2cTarget: b.i2c_target,
        drawing: b.drawing,
      };
    } finally {
      b.free();
    }
  });
}

export function encodeHelloReq(r: HelloReq): Uint8Array {
  return hello_request(r.protoVer, r.maxPayload);
}

/** Body after the status byte. Throws on a malformed body. */
export function decodeHelloResp(body: Uint8Array): HelloResp {
  const h = hello_decode(body);
  try {
    return {
      protoVer: h.proto_ver,
      role: h.role,
      portKind: h.port_kind,
      mcuId: h.mcu_id,
      mode: h.mode,
      activeVersion: h.active_version,
      trial: h.trial,
      keyId: h.key_id,
      maxPayload: h.max_payload,
    };
  } finally {
    h.free();
  }
}

/** `major.minor.patch` of a raw version u32. */
export function versionText(raw: number): string {
  return version_text(raw);
}
