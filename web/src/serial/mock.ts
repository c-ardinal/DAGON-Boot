// In-memory PC-connected node for development and tests: the sample PC-connected node plus the
// 7-segment node (I2C 0x42) and the watch (UART), answering in the
// dagon-proto frame format. It covers what the page drives: HELLO, STATUS,
// RELAY to the two nodes and the update steps
// (XFER_BEGIN/HDR/DATA/END, SET_PENDING, REBOOT, CONFIRM, SHOW). Images are
// not checked here; dagon-host checks the release before sending. Every
// request is read by dagon-proto's reader before the node connected to the PC acts on it, and
// every response is written by its writer (dagon-wasm): no command, status
// or field number and no payload layout is written here.
import { CMD, encodeFrame, parseFrame, STATUS, WIRE } from './frame.ts';
import { encodeHelloResp, versionRaw, type HelloResp } from './hello.ts';
import { FrameQueue, type Transport } from './transport.ts';
import sampleYaml from '../../../topology/sample.yaml?raw';
import { HELLO_MODE, HELLO_ROLE, PORT_KIND, SLOT_STATE, readRequest, respond, servedCmds, topo_targets, wireVersion, type Target, type WireRequest } from '../wasm/dagon.ts';

const HDR_VERSION_AT = 0x018;

const KEY_ID = Uint8Array.of(0x3f, 0x9a, 0x21, 0xc7, 0x00, 0x00, 0x00, 0x00);
const V100 = versionRaw(1, 0, 0);

/** The sample PC-connected node: 1.0.0, confirmed, sample key id. */
export const SAMPLE_HUB_HELLO: HelloResp = {
  get protoVer() {
    return wireVersion();
  },
  get role() {
    return HELLO_ROLE.hub;
  },
  get portKind() {
    return PORT_KIND.selfBoot;
  },
  mcuId: 1,
  get mode() {
    return HELLO_MODE.app;
  },
  activeVersion: V100,
  trial: false,
  keyId: KEY_ID,
  get maxPayload() {
    return WIRE.MAX_PAYLOAD_USB;
  },
  get cmds() {
    return servedCmds(true);
  },
};

const node = (portKind: number, mcuId: number, maxPayload: number): HelloResp => ({
  ...SAMPLE_HUB_HELLO,
  role: HELLO_ROLE.node,
  portKind,
  mcuId,
  maxPayload,
  cmds: servedCmds(false),
});

interface Slot {
  state: number;
  version: number;
}

interface Device {
  hello: HelloResp;
  /** A, B, factory. */
  slots: Slot[];
  active: number;
  confirmed: number;
  /** The last reboot fell back to the previous slot. */
  fellBack?: boolean;
  /** Transfer in progress: target slot, header and body bytes. */
  xfer: { target: number; slot: number; hdr: number[]; body: number[] } | null;
  /** Slot to run on the next REBOOT. */
  pending: number | null;
}

/** Device keys: `hub`, `i2c:<addr>`, `uart`. */
export type DeviceKey = 'hub' | `i2c:${number}` | 'uart';

/** The device of each linked node of the sample configuration. */
const SAMPLE_DEVICE: Record<string, DeviceKey> = { seg: 'i2c:66', watch: 'uart' };

let sampleTargets: Record<string, Target> | undefined;

/** The device a RELAY over `link` to `addr` reaches: by the sample's targets as dagon-host gives them (read once dagon-wasm runs). */
function deviceAt(link: number, addr: number): DeviceKey | null {
  sampleTargets ??= topo_targets(sampleYaml);
  const hit = Object.entries(sampleTargets).find(([, t]) => t.kind === 'linked' && t.link === link && t.addr === addr);
  return hit ? (SAMPLE_DEVICE[hit[0]] ?? null) : null;
}

const aConfirmed = (): Slot[] => [
  { state: SLOT_STATE.confirmed, version: V100 },
  { state: SLOT_STATE.empty, version: 0 },
  { state: SLOT_STATE.empty, version: 0 },
];

function device(hello: HelloResp, slots = aConfirmed()): Device {
  return { hello, slots, active: 0, confirmed: 0, xfer: null, pending: null };
}

/** Start state of the sample: every device on 1.0.0 in slot A; the watch also has its factory image. */
function sampleDevices(hub: HelloResp): Map<DeviceKey, Device> {
  const watch = aConfirmed();
  watch[2] = { state: SLOT_STATE.written, version: V100 };
  return new Map<DeviceKey, Device>([
    ['hub', device(hub)],
    ['i2c:66', device(node(PORT_KIND.selfBoot, 2, WIRE.MAX_PAYLOAD_I2C))],
    ['uart', device(node(PORT_KIND.vendorStage2, 3, WIRE.MAX_PAYLOAD_UART), watch)],
  ]);
}

export interface MockOptions {
  hello?: HelloResp;
  /** Never answer (a device that is not a node connected to the PC). */
  silent?: boolean;
  /** Delay of the PC-connected node's HELLO reply, to make the checking state visible. */
  delayMs?: number;
  /** Nodes behind the node connected to the PC that do not answer RELAY. */
  quiet?: DeviceKey[];
  /** Devices answering as their factory agent (HELLO mode 3) until the next REBOOT. */
  factory?: DeviceKey[];
  /** Devices that refuse the image at XFER_END with this status (0x07 E_FLASH, 0x09 E_SIGNATURE, ...). */
  refuse?: Partial<Record<DeviceKey, number>>;
  /** Devices that come back on their previous version after REBOOT (the new one did not start). */
  fallBack?: DeviceKey[];
  /**
   * Opens that fail after the PC-connected node's REBOOT, while its USB device is away
   * (default 2). `Infinity`: it stays away until `replug()`, as when the
   * browser cannot reopen it without the user choosing it again.
   */
  hubAway?: number;
}

export class MockTransport implements Transport {
  private readonly queue = new FrameQueue();
  /** Made when first used: the wire's numbers come from dagon-wasm, loaded by then. */
  private made?: Map<DeviceKey, Device>;
  private readonly quiet: Set<DeviceKey>;
  private isOpen = false;
  private refuse: Partial<Record<DeviceKey, number>>;
  private hubAway: number;
  /** Devices that took a REBOOT, in order. */
  readonly reboots: DeviceKey[] = [];
  /** Failing opens left while the rebooting PC-connected node is away. */
  private away = 0;
  constructor(private readonly opts: MockOptions = {}) {
    this.quiet = new Set(opts.quiet);
    this.refuse = { ...opts.refuse };
    this.hubAway = opts.hubAway ?? 2;
  }

  private get devices(): Map<DeviceKey, Device> {
    if (this.made) return this.made;
    const made = sampleDevices(this.opts.hello ?? SAMPLE_HUB_HELLO);
    for (const k of this.opts.factory ?? []) {
      const d = made.get(k);
      if (d) d.hello = { ...d.hello, mode: HELLO_MODE.agent };
    }
    this.made = made;
    return made;
  }

  /** Makes a node stop or start answering. */
  setQuiet(key: DeviceKey, quiet: boolean): void {
    if (quiet) this.quiet.add(key);
    else this.quiet.delete(key);
  }

  /** Makes a device refuse the image at XFER_END with `status`, or stop refusing. */
  setRefuse(key: DeviceKey, status?: number): void {
    this.refuse = { ...this.refuse, [key]: status };
  }

  /** Opens that fail after the PC-connected node's next REBOOT (`Infinity`: until `replug()`). */
  setHubAway(opens: number): void {
    this.hubAway = opens;
  }

  /** The rebooted PC-connected node's USB device is back. */
  replug(): void {
    this.away = 0;
  }

  /** HELLO of a device as it answers now. */
  helloOf(key: DeviceKey): HelloResp {
    return helloOf(this.devices.get(key)!);
  }

  async open(): Promise<void> {
    if (this.away > 0) {
      this.away--;
      throw new DOMException('device away', 'NetworkError');
    }
    this.isOpen = true;
  }

  async close(): Promise<void> {
    this.isOpen = false;
    this.queue.clear();
  }

  async write(raw: Uint8Array): Promise<void> {
    if (!this.isOpen) throw new Error('port not open');
    if (this.opts.silent) return;
    const frame = parseFrame(raw);
    const req = readRequest(frame.cmd, frame.payload, true);
    const hub = this.devices.get('hub')!;
    const body = req.refusal ?? (frame.cmd === CMD.RELAY ? await this.relay(req) : await answer(hub, frame.cmd, req, this.fellBack('hub'), this.refuse.hub));
    const resp = encodeFrame({ flags: WIRE.FLAG_RESP, seq: frame.seq, cmd: frame.cmd, payload: body });
    // Only the PC-connected node's own HELLO waits, so the checking state shows without slowing a scan.
    setTimeout(() => this.queue.put(resp), frame.cmd === CMD.HELLO ? (this.opts.delayMs ?? 0) : 0);
    if (frame.cmd === CMD.REBOOT && body[0] === STATUS.OK) {
      this.reboots.push('hub');
      this.away = this.hubAway;
    }
  }

  read(timeoutMs: number): Promise<Uint8Array | null> {
    return this.queue.take(timeoutMs);
  }

  private fellBack(key: DeviceKey): boolean {
    return this.opts.fallBack?.includes(key) ?? false;
  }

  private async relay(req: WireRequest): Promise<Uint8Array> {
    const key = deviceAt(req.link, req.addr);
    const dev = key && !this.quiet.has(key) ? this.devices.get(key) : undefined;
    if (!key || !dev) return respond.status(STATUS.TIMEOUT);
    const inner = parseFrame(req.bulk);
    const innerReq = readRequest(inner.cmd, inner.payload, false);
    const body = innerReq.refusal ?? (await answer(dev, inner.cmd, innerReq, this.fellBack(key), this.refuse[key]));
    if (inner.cmd === CMD.REBOOT && body[0] === STATUS.OK) this.reboots.push(key);
    const resp = encodeFrame({ flags: WIRE.FLAG_RESP, seq: inner.seq, cmd: inner.cmd, payload: body });
    return respond.relay(resp);
  }
}

function helloOf(dev: Device): HelloResp {
  return { ...dev.hello, activeVersion: dev.slots[dev.active].version, trial: dev.active !== dev.confirmed };
}

/** Answers a request dagon-proto's reader accepted (`req`): the response payload. */
async function answer(dev: Device, cmd: number, req: WireRequest, fallBack: boolean, refuse?: number): Promise<Uint8Array> {
  const ok = respond.status(STATUS.OK);
  const state = respond.status(STATUS.STATE);
  switch (cmd) {
    case CMD.HELLO:
      return Uint8Array.of(STATUS.OK, ...encodeHelloResp(helloOf(dev)));
    case CMD.STATUS:
      return encodeStatus(dev);
    case CMD.XFER_BEGIN:
      dev.xfer = { target: req.target, slot: req.slot, hdr: [], body: [] };
      return ok;
    case CMD.XFER_HDR:
    case CMD.XFER_DATA:
      return chunk(dev, cmd, req);
    case CMD.XFER_END:
      if (refuse !== undefined) {
        dev.xfer = null;
        return respond.status(refuse);
      }
      return end(dev);
    case CMD.SET_PENDING:
      if (dev.slots[req.slot]?.state !== SLOT_STATE.written) return state;
      dev.pending = req.slot;
      dev.slots[req.slot].state = SLOT_STATE.pending;
      return ok;
    case CMD.REBOOT:
      reboot(dev, fallBack);
      return ok;
    case CMD.CONFIRM:
      if (dev.active === dev.confirmed) return state;
      dev.slots[dev.confirmed].state = SLOT_STATE.written;
      dev.slots[dev.active].state = SLOT_STATE.confirmed;
      dev.confirmed = dev.active;
      return ok;
    case CMD.SHOW:
      return ok;
    default:
      return respond.status(STATUS.UNSUPPORTED);
  }
}

function chunk(dev: Device, cmd: number, req: WireRequest): Uint8Array {
  const x = dev.xfer;
  if (!x) return respond.status(STATUS.STATE);
  const into = cmd === CMD.XFER_HDR ? x.hdr : x.body;
  if (req.offset !== into.length) return respond.status(STATUS.ARG);
  into.push(...req.bulk);
  return respond.nextOffset(into.length);
}

async function end(dev: Device): Promise<Uint8Array> {
  const x = dev.xfer;
  if (!x) return respond.status(STATUS.STATE);
  dev.xfer = null;
  const body = new Uint8Array(x.body);
  const version = new DataView(new Uint8Array(x.hdr).buffer).getUint32(HDR_VERSION_AT, true);
  dev.slots[x.slot] = { state: SLOT_STATE.written, version };
  return respond.digest(new Uint8Array(await crypto.subtle.digest('SHA-256', body)));
}

/** Runs the pending slot on trial; a device that falls back stays on its previous slot. */
function reboot(dev: Device, fallBack: boolean): void {
  const next = dev.pending;
  dev.pending = null;
  dev.hello = { ...dev.hello, mode: HELLO_MODE.app };
  if (next === null) return;
  if (fallBack) {
    dev.slots[next].state = SLOT_STATE.reverted;
    dev.fellBack = true;
    return;
  }
  dev.active = next;
  dev.slots[next].state = SLOT_STATE.trying;
}

const LOG_GENERATION = 12;
/** STATUS last_reason after a fall back (dagon_format::statelog::reason). */
const REASON_TRIAL_EXHAUSTED = 3;
const DIGEST16_LEN = 16;

/** The OK STATUS response of `dev`: a written slot shows a digest of its own. */
function encodeStatus(dev: Device): Uint8Array {
  const slots = dev.slots.map((s, i) => ({
    state: s.state,
    version: s.version,
    digest16: new Uint8Array(DIGEST16_LEN).fill(s.state === SLOT_STATE.empty ? 0 : 0xa4 + i),
  }));
  return respond.slots(slots, {
    active: dev.active,
    confirmed: dev.confirmed,
    attempts: 0,
    lastReason: dev.fellBack ? REASON_TRIAL_EXHAUSTED : 0,
    logGeneration: LOG_GENERATION,
  });
}
