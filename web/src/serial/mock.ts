// In-memory desk hub for development and tests: the sample hub plus the
// 7-segment node (I2C 0x42) and the watch (UART), answering in the
// dagon-proto frame format. It covers what the page drives: HELLO, STATUS,
// RELAY to the two nodes, the topology transfer and the update steps
// (XFER_BEGIN/HDR/DATA/END, SET_PENDING, REBOOT, CONFIRM, SHOW). Images are
// not checked here; dagon-host checks the release before sending.
import { CMD, FLAG_RESP, MAX_PAYLOAD_USB, STATUS_OK, encodeFrame, parseFrame } from './frame.ts';
import { ROLE_HUB, ROLE_NODE, encodeHelloResp, versionRaw, type HelloResp } from './hello.ts';
import { FrameQueue, type Transport } from './transport.ts';

const STATUS_UNSUPPORTED = 0x02;
const STATUS_ARG = 0x03;
const STATUS_STATE = 0x05;
const STATUS_TIMEOUT = 0x0f;
const CMD_XFER_BEGIN = 0x10;
const CMD_XFER_DATA = 0x11;
const CMD_XFER_END = 0x12;
const CMD_XFER_HDR = 0x15;
const CMD_SET_PENDING = 0x20;
const CMD_REBOOT = 0x21;
const CMD_CONFIRM = 0x22;
const CMD_SHOW = 0x41;
const CMD_RELAY = 0x50;
const TARGET_TOPOLOGY = 3;
const LINK_I2C0 = 1;
const LINK_PIO_UART = 2;
const RELAY_HEAD_LEN = 4;
const CHUNK_HEAD = 4;
const HDR_VERSION_AT = 0x018;
const MAX_PAYLOAD_I2C = 256;
const MAX_PAYLOAD_UART = 1024;
const SLOT_EMPTY = 0;
const SLOT_WRITTEN = 1;
const SLOT_PENDING = 2;
const SLOT_TRYING = 3;
const SLOT_CONFIRMED = 4;
const MODE_APP = 0;
const MODE_AGENT = 3;

const KEY_ID = Uint8Array.of(0x3f, 0x9a, 0x21, 0xc7, 0x00, 0x00, 0x00, 0x00);
const V100 = versionRaw(1, 0, 0);

/** The sample hub: 1.0.0, confirmed, sample key id. */
export const SAMPLE_HUB_HELLO: HelloResp = {
  protoVer: 1,
  role: ROLE_HUB,
  portKind: 1,
  mcuId: 1,
  mode: 0,
  activeVersion: V100,
  trial: false,
  keyId: KEY_ID,
  maxPayload: MAX_PAYLOAD_USB,
};

const node = (portKind: number, mcuId: number, maxPayload: number): HelloResp => ({
  ...SAMPLE_HUB_HELLO,
  role: ROLE_NODE,
  portKind,
  mcuId,
  maxPayload,
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
  /** Transfer in progress: target slot (or the topology), header and body bytes. */
  xfer: { target: number; slot: number; hdr: number[]; body: number[] } | null;
  /** Slot to run on the next REBOOT. */
  pending: number | null;
  /** The hub's display copy of the topology. */
  topology: Uint8Array | null;
}

/** Device keys: `hub`, `i2c:<addr>`, `uart`. */
export type DeviceKey = 'hub' | `i2c:${number}` | 'uart';

const aConfirmed = (): Slot[] => [
  { state: SLOT_CONFIRMED, version: V100 },
  { state: SLOT_EMPTY, version: 0 },
  { state: SLOT_EMPTY, version: 0 },
];

function device(hello: HelloResp, slots = aConfirmed()): Device {
  return { hello, slots, active: 0, confirmed: 0, xfer: null, pending: null, topology: null };
}

/** Start state of the sample: every device on 1.0.0 in slot A; the watch also has its factory image. */
function deskDevices(hub: HelloResp): Map<DeviceKey, Device> {
  const watch = aConfirmed();
  watch[2] = { state: SLOT_WRITTEN, version: V100 };
  return new Map<DeviceKey, Device>([
    ['hub', device(hub)],
    ['i2c:66', device(node(1, 2, MAX_PAYLOAD_I2C))],
    ['uart', device(node(2, 3, MAX_PAYLOAD_UART), watch)],
  ]);
}

export interface MockOptions {
  hello?: HelloResp;
  /** Never answer (a device that is not a hub). */
  silent?: boolean;
  /** Delay of the hub's HELLO reply, to make the checking state visible. */
  delayMs?: number;
  /** Nodes behind the hub that do not answer RELAY. */
  quiet?: DeviceKey[];
  /** Devices answering as their factory agent (HELLO mode 3) until the next REBOOT. */
  factory?: DeviceKey[];
  /** Devices that refuse the image at XFER_END with this status (0x07 E_FLASH, 0x09 E_SIGNATURE, ...). */
  refuse?: Partial<Record<DeviceKey, number>>;
  /** Devices that come back on their previous version after REBOOT (the new one did not start). */
  fallBack?: DeviceKey[];
  /**
   * Opens that fail after the hub's REBOOT, while its USB device is away
   * (default 2). `Infinity`: it stays away until `replug()`, as when the
   * browser cannot reopen it without the user choosing it again.
   */
  hubAway?: number;
}

export class MockTransport implements Transport {
  private readonly queue = new FrameQueue();
  private readonly devices: Map<DeviceKey, Device>;
  private readonly quiet: Set<DeviceKey>;
  private isOpen = false;
  private refuse: Partial<Record<DeviceKey, number>>;
  /** Devices that took a REBOOT, in order. */
  readonly reboots: DeviceKey[] = [];
  /** Failing opens left while the rebooting hub is away. */
  private away = 0;
  /** The last topology the hub received in full. */
  get received(): Uint8Array | null {
    return this.devices.get('hub')!.topology;
  }

  constructor(private readonly opts: MockOptions = {}) {
    this.devices = deskDevices(opts.hello ?? SAMPLE_HUB_HELLO);
    this.quiet = new Set(opts.quiet);
    this.refuse = { ...opts.refuse };
    for (const k of opts.factory ?? []) {
      const d = this.devices.get(k);
      if (d) d.hello = { ...d.hello, mode: MODE_AGENT };
    }
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

  /** The rebooted hub's USB device is back. */
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
    const req = parseFrame(raw);
    const hub = this.devices.get('hub')!;
    const body = req.cmd === CMD_RELAY ? await this.relay(req.payload) : await answer(hub, req.cmd, req.payload, this.fellBack('hub'), this.refuse.hub);
    const resp = encodeFrame({ flags: FLAG_RESP, seq: req.seq, cmd: req.cmd, payload: body });
    // Only the hub's own HELLO waits, so the checking state shows without slowing a scan.
    setTimeout(() => this.queue.put(resp), req.cmd === CMD.HELLO ? (this.opts.delayMs ?? 0) : 0);
    if (req.cmd === CMD_REBOOT && body[0] === STATUS_OK) {
      this.reboots.push('hub');
      this.away = this.opts.hubAway ?? 2;
    }
  }

  read(timeoutMs: number): Promise<Uint8Array | null> {
    return this.queue.take(timeoutMs);
  }

  private fellBack(key: DeviceKey): boolean {
    return this.opts.fallBack?.includes(key) ?? false;
  }

  private async relay(p: Uint8Array): Promise<Uint8Array> {
    if (p.length < RELAY_HEAD_LEN) return Uint8Array.of(STATUS_ARG);
    const [link, addr] = p;
    const key: DeviceKey | null = link === LINK_I2C0 ? `i2c:${addr}` : link === LINK_PIO_UART ? 'uart' : null;
    const dev = key && !this.quiet.has(key) ? this.devices.get(key) : undefined;
    if (!key || !dev) return Uint8Array.of(STATUS_TIMEOUT);
    const inner = parseFrame(p.subarray(RELAY_HEAD_LEN));
    const body = await answer(dev, inner.cmd, inner.payload, this.fellBack(key), this.refuse[key]);
    if (inner.cmd === CMD_REBOOT) this.reboots.push(key);
    const resp = encodeFrame({ flags: FLAG_RESP, seq: inner.seq, cmd: inner.cmd, payload: body });
    return withStatus(STATUS_OK, resp);
  }
}

function helloOf(dev: Device): HelloResp {
  return { ...dev.hello, activeVersion: dev.slots[dev.active].version, trial: dev.active !== dev.confirmed };
}

const SLOT_REVERTED = 5;

async function answer(dev: Device, cmd: number, p: Uint8Array, fallBack: boolean, refuse?: number): Promise<Uint8Array> {
  switch (cmd) {
    case CMD.HELLO:
      return withStatus(STATUS_OK, encodeHelloResp(helloOf(dev)));
    case CMD.STATUS:
      return withStatus(STATUS_OK, encodeStatus(dev));
    case CMD_XFER_BEGIN:
      dev.xfer = { target: p[0], slot: p[1], hdr: [], body: [] };
      return Uint8Array.of(STATUS_OK);
    case CMD_XFER_HDR:
    case CMD_XFER_DATA:
      return chunk(dev, cmd, p);
    case CMD_XFER_END:
      if (refuse !== undefined && dev.xfer?.target !== TARGET_TOPOLOGY) {
        dev.xfer = null;
        return Uint8Array.of(refuse);
      }
      return end(dev);
    case CMD_SET_PENDING:
      if (dev.slots[p[1]]?.state !== SLOT_WRITTEN) return Uint8Array.of(STATUS_STATE);
      dev.pending = p[1];
      dev.slots[p[1]].state = SLOT_PENDING;
      return Uint8Array.of(STATUS_OK);
    case CMD_REBOOT:
      reboot(dev, fallBack);
      return Uint8Array.of(STATUS_OK);
    case CMD_CONFIRM:
      if (dev.active === dev.confirmed) return Uint8Array.of(STATUS_STATE);
      dev.slots[dev.confirmed].state = SLOT_WRITTEN;
      dev.slots[dev.active].state = SLOT_CONFIRMED;
      dev.confirmed = dev.active;
      return Uint8Array.of(STATUS_OK);
    case CMD_SHOW:
      return Uint8Array.of(STATUS_OK);
    default:
      return Uint8Array.of(STATUS_UNSUPPORTED);
  }
}

function chunk(dev: Device, cmd: number, p: Uint8Array): Uint8Array {
  const x = dev.xfer;
  if (!x || p.length < CHUNK_HEAD) return Uint8Array.of(STATUS_STATE);
  const into = cmd === CMD_XFER_HDR ? x.hdr : x.body;
  const offset = new DataView(p.buffer, p.byteOffset).getUint32(0, true);
  if (offset !== into.length) return Uint8Array.of(STATUS_ARG);
  into.push(...p.subarray(CHUNK_HEAD));
  return withStatus(STATUS_OK, u32(into.length));
}

async function end(dev: Device): Promise<Uint8Array> {
  const x = dev.xfer;
  if (!x) return Uint8Array.of(STATUS_STATE);
  dev.xfer = null;
  const body = new Uint8Array(x.body);
  if (x.target === TARGET_TOPOLOGY) {
    dev.topology = body;
  } else {
    const version = new DataView(new Uint8Array(x.hdr).buffer).getUint32(HDR_VERSION_AT, true);
    dev.slots[x.slot] = { state: SLOT_WRITTEN, version };
  }
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', body));
  return withStatus(STATUS_OK, digest);
}

/** Runs the pending slot on trial; a device that falls back stays on its previous slot. */
function reboot(dev: Device, fallBack: boolean): void {
  const next = dev.pending;
  dev.pending = null;
  dev.hello = { ...dev.hello, mode: MODE_APP };
  if (next === null) return;
  if (fallBack) {
    dev.slots[next].state = SLOT_REVERTED;
    return;
  }
  dev.active = next;
  dev.slots[next].state = SLOT_TRYING;
}

const STATUS_LEN = 71;
const SLOT_LEN = 21;
const LOG_GENERATION = 12;

/** STATUS body: 3 × {state, version, digest16}, active, confirmed, attempts, last_reason, log_generation. */
function encodeStatus(dev: Device): Uint8Array {
  const out = new Uint8Array(STATUS_LEN);
  const dv = new DataView(out.buffer);
  dev.slots.forEach((s, i) => {
    out[i * SLOT_LEN] = s.state;
    dv.setUint32(i * SLOT_LEN + 1, s.version, true);
    if (s.state !== SLOT_EMPTY) out.fill(0xa4 + i, i * SLOT_LEN + 5, i * SLOT_LEN + SLOT_LEN);
  });
  const tail = 3 * SLOT_LEN;
  out.set([dev.active, dev.confirmed, 0, 0], tail);
  dv.setUint32(tail + 4, LOG_GENERATION, true);
  return out;
}

function u32(v: number): Uint8Array {
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, v, true);
  return out;
}

function withStatus(status: number, body: Uint8Array): Uint8Array {
  const out = new Uint8Array(1 + body.length);
  out[0] = status;
  out.set(body, 1);
  return out;
}
