// The page's single entry to dagon-wasm (built into ./pkg by `npm run wasm`).
// Converts the generated classes into plain objects so nothing outside this
// file has to free WASM memory.
import init, {
  FrameCutter,
  WasmHost,
  frame_encode,
  hello_decode,
  hello_request,
  load_definitions,
  mcu_connection,
  release_versions,
  use_builtin_definitions,
  supported_boards as boards,
  version_older,
  version_text,
} from './pkg/dagon_wasm.js';
import type { HostLink } from './pkg/dagon_wasm.js';
import type { HelloReq, HelloResp } from '../serial/hello.ts';

export { check_release_for, initSync, release_topology, require_release_key, set_release_key, topo_check, topo_compile, verify_release, watch_interval_ms } from './pkg/dagon_wasm.js';

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

/** Progress of an update (dagon-host's Event; `phase` is the SHOW phase). */
export interface UpdateEvent {
  /** Topology node index. */
  node: number;
  phase: number;
  percent: number;
  version: number;
}

/** SHOW phases (dagon_proto::payload::phase). */
export const PHASE = { transfer: 1, verify: 2, reboot: 3, confirm: 4, rejected: 5, failed: 6, rom: 7 } as const;

export interface NodeFailure {
  /** dagon-host's short reason: `NO ANSWER`, `ROLLED BACK`, `E_SIGNATURE`, ... */
  reason: string;
  /** Version the node came back on, when it is not the new one. */
  running: number | null;
  trial: boolean;
}

export interface NodeResult {
  node: number;
  kind: TargetKind;
  addr: number;
  outcome: 'done' | 'failed' | 'notStarted';
  version: number | null;
  failure: NodeFailure | null;
}

export interface ConfirmResult {
  outcome: 'confirmed' | 'notOnTrial' | 'needsChoice';
  version: number;
}

/** ROM-path figures of a recovery (dagon-romlink's Stats). */
export interface RomStats {
  attempts: number;
  frames: number;
  maxRttMs: number;
  elapsedMs: number;
  noStretch: boolean;
}

export interface RecoverResult {
  /** The normal transfer to the app, to the factory agent, or the ROM loader. */
  path: 'retransfer' | 'factory' | 'rom';
  version: number;
  rom: RomStats | null;
}

/** Why a release is refused before anything is sent (dagon-host's check). */
export type Refusal = 'source' | 'key' | 'topology' | 'format';

/** dagon-host's procedures for one hub session, with the shapes dagon-wasm returns. */
export interface Host {
  hello(kind: TargetKind, addr: number): Promise<HelloResp>;
  status(kind: TargetKind, addr: number): Promise<NodeState>;
  discover(): Promise<Discovery>;
  pushTopology(bin: Uint8Array): Promise<void>;
  watch(kind: TargetKind, addr: number): Promise<WatchRound>;
  /** `dagon apply`; rejects with a Refusal (or a reason) when nothing was sent. */
  apply(bundle: Uint8Array, topology: Uint8Array, nodes: number[] | null, onEvent: (e: UpdateEvent) => void): Promise<NodeResult[]>;
  confirm(kind: TargetKind, addr: number, force: boolean): Promise<ConfirmResult>;
  /** `dagon recover` of topology node `node`; rejects with a Refusal or dagon-host's reason. */
  recover(node: number, release: Uint8Array, topology: Uint8Array, bootcore: Uint8Array | null, onEvent: (e: UpdateEvent) => void): Promise<RecoverResult>;
  /** Back to the version in the other slot; resolves to it. */
  rollback(kind: TargetKind, addr: number, node: number, onEvent: (e: UpdateEvent) => void): Promise<number>;
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
    apply: (b, t, n, on) => h.apply(b, t, n ? Uint8Array.from(n) : undefined, on),
    confirm: (k, a, f) => h.confirm(k, a, f),
    recover: (n, r, t, b, on) => h.recover(n, r, t, b ?? undefined, on),
    rollback: (k, a, n, on) => h.rollback(k, a, n, on),
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

/** No MCU and no board: what the page works with while a project's definitions do not load. */
const NO_DEFINITIONS = '{"mcus":[],"boards":[]}';

/**
 * Uses the MCU and board definitions `dagon serve` sends (GET /v1/definitions):
 * - 'loaded': they loaded;
 * - 'builtin': serve sent none (no serve, ?mock, an older serve): the builtin set;
 * - 'blocked': the open project's definitions failed (`failed`: serve said 500) or what serve
 *   sent does not load: no MCU or board at all, never the builtin set in their place.
 * Nothing of a previous project stays in any case.
 */
export function useDefinitions(json: string | null, failed = false): 'loaded' | 'builtin' | 'blocked' {
  if (!failed && json === null) {
    use_builtin_definitions();
    return 'builtin';
  }
  try {
    if (json !== null && !failed) {
      load_definitions(json);
      return 'loaded';
    }
  } catch {
    // Falls through to the blocked state.
  }
  load_definitions(NO_DEFINITIONS);
  return 'blocked';
}

/** How the user connects `mcu` for a write (its definition's [connection]); null for an unknown MCU. */
export function mcuConnection(mcu: string): { label: string; hint: string } | null {
  return (mcu_connection(mcu) as { label: string; hint: string } | null) ?? null;
}

export interface Board {
  id: string;
  product: string;
  mcuName: string;
  mcu: string;
  flashBytes: number;
  i2cTarget: boolean;
  drawing: string;
  /** The board a node naming only its MCU stands for (the definitions' rule, not list order). */
  isDefault: boolean;
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
        isDefault: b.is_default,
      };
    } finally {
      b.free();
    }
  });
}

/** The board a node naming only `mcu` stands for; undefined when the MCU has none (several boards, none marked). */
export function defaultBoard(mcu: string): string | undefined {
  return supportedBoards().find((b) => b.mcu === mcu && b.isDefault)?.id;
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

/** Raw version `a` is older than `b`, by dagon-format's ordering. */
export function versionOlder(a: number, b: number): boolean {
  return version_older(a, b);
}

/** The version a release gives each topology node (by node index); empty when it cannot be read. */
export function releaseVersions(bundle: Uint8Array): Map<number, number> {
  const flat = release_versions(bundle);
  const out = new Map<number, number>();
  for (let i = 0; i + 1 < flat.length; i += 2) if (!out.has(flat[i])) out.set(flat[i], flat[i + 1]);
  return out;
}

/** `major.minor.patch` of a raw version u32. */
export function versionText(raw: number): string {
  return version_text(raw);
}
