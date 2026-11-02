// The page's single entry to dagon-wasm (built into ./pkg by `npm run wasm`).
// Converts the generated classes into plain objects so nothing outside this
// file has to free WASM memory.
import init, {
  FrameCutter,
  WasmHost,
  frame_encode,
  hello_decode,
  hello_request,
  supported_boards as boards,
  version_text,
} from './pkg/dagon_wasm.js';
import type { HostLink } from './pkg/dagon_wasm.js';
import type { HelloReq, HelloResp } from '../serial/hello.ts';

export { initSync, topo_check, topo_compile, verify_release, watch_interval_ms } from './pkg/dagon_wasm.js';

export type TargetKind = 'hub' | 'i2c' | 'uart';

export interface SlotInfo {
  /** 0 empty, 1 written, 2 next to try, 3 on trial, 4 confirmed, 5 rolled back. */
  state: number;
  version: number;
  digest16: Uint8Array;
}

export interface StatusResp {
  /** A, B, factory. */
  slots: [SlotInfo, SlotInfo, SlotInfo];
  /** Running slot, 0xFF none. */
  active: number;
  confirmed: number;
  attempts: number;
  lastReason: number;
  logGeneration: number;
}

export interface NodeState {
  hello: HelloResp;
  status: StatusResp;
}

export interface WatchRound {
  /** False only after two misses in a row. */
  responding: boolean;
  rttMs: number | null;
  hello: HelloResp | null;
}

export interface Discovery {
  hub: HelloResp;
  nodes: { kind: TargetKind; addr: number; hello: HelloResp }[];
}

/** dagon-host's procedures for one hub session, with the shapes dagon-wasm returns. */
export interface Host {
  hello(kind: TargetKind, addr: number): Promise<HelloResp>;
  status(kind: TargetKind, addr: number): Promise<NodeState>;
  discover(): Promise<Discovery>;
  pushTopology(bin: Uint8Array): Promise<void>;
  watch(kind: TargetKind, addr: number): Promise<WatchRound>;
  /** Releases the WASM side; calls still running finish on their own. */
  free(): void;
}

export function newHost(link: HostLink): Host {
  const h = new WasmHost(link);
  return {
    hello: (k, a) => h.hello(k, a),
    status: (k, a) => h.status(k, a),
    discover: () => h.discover(),
    pushTopology: (bin) => h.push_topology(bin),
    watch: (k, a) => h.watch(k, a),
    free: () => h.free(),
  };
}

/** A complete link frame (header, payload, CRC), encoded by dagon-proto. */
export function encodeFrameBytes(flags: number, seq: number, cmd: number, payload: Uint8Array): Uint8Array {
  return frame_encode(flags, seq, cmd, payload);
}

/** Stream cutter: SOF search, length and CRC checks are dagon-proto's. */
export interface FrameStream {
  /** Each complete, CRC-checked frame in `chunk` (and earlier partial bytes). */
  push(chunk: Uint8Array): Uint8Array[];
  /** Releases the WASM side. */
  free(): void;
}

export function frameStream(maxPayload: number): FrameStream {
  const c = new FrameCutter(maxPayload);
  return {
    push: (chunk) => Array.from(c.push(chunk) as Uint8Array[]),
    free: () => c.free(),
  };
}

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
