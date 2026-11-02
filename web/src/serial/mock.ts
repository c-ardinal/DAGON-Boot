// In-memory desk hub for development and tests: the sample hub plus the
// 7-segment node (I2C 0x42) and the watch (UART), answering in the
// dagon-proto frame format. It covers what the page drives today: HELLO,
// STATUS, RELAY to the two nodes and the topology transfer.
import { CMD, FLAG_RESP, MAX_PAYLOAD_USB, STATUS_OK, encodeFrame, parseFrame } from './frame.ts';
import { ROLE_HUB, ROLE_NODE, encodeHelloResp, versionRaw, type HelloResp } from './hello.ts';
import { FrameQueue, type Transport } from './transport.ts';

const STATUS_UNSUPPORTED = 0x02;
const STATUS_ARG = 0x03;
const STATUS_TIMEOUT = 0x0f;
const CMD_XFER_BEGIN = 0x10;
const CMD_XFER_DATA = 0x11;
const CMD_XFER_END = 0x12;
const CMD_RELAY = 0x50;
const LINK_I2C0 = 1;
const LINK_PIO_UART = 2;
const RELAY_HEAD_LEN = 4;
const MAX_PAYLOAD_I2C = 256;
const MAX_PAYLOAD_UART = 1024;
const SLOT_EMPTY = 0;
const SLOT_WRITTEN = 1;
const SLOT_CONFIRMED = 4;

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
  slots: [Slot, Slot, Slot];
}

/** Device keys: `hub`, `i2c:<addr>`, `uart`. */
export type DeviceKey = 'hub' | `i2c:${number}` | 'uart';

const A_CONFIRMED: [Slot, Slot, Slot] = [
  { state: SLOT_CONFIRMED, version: V100 },
  { state: SLOT_EMPTY, version: 0 },
  { state: SLOT_EMPTY, version: 0 },
];

/** Start state of the sample: every device on 1.0.0 in slot A; the watch also has its factory image. */
function deskDevices(hub: HelloResp): Map<DeviceKey, Device> {
  return new Map<DeviceKey, Device>([
    ['hub', { hello: hub, slots: A_CONFIRMED }],
    ['i2c:66', { hello: node(1, 2, MAX_PAYLOAD_I2C), slots: A_CONFIRMED }],
    ['uart', { hello: node(2, 3, MAX_PAYLOAD_UART), slots: [A_CONFIRMED[0], A_CONFIRMED[1], { state: SLOT_WRITTEN, version: V100 }] }],
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
}

export class MockTransport implements Transport {
  private readonly queue = new FrameQueue();
  private readonly devices: Map<DeviceKey, Device>;
  private readonly quiet: Set<DeviceKey>;
  private isOpen = false;
  private topology: number[] = [];
  /** The last topology the hub received in full. */
  received: Uint8Array | null = null;

  constructor(private readonly opts: MockOptions = {}) {
    this.devices = deskDevices(opts.hello ?? SAMPLE_HUB_HELLO);
    this.quiet = new Set(opts.quiet);
  }

  /** Makes a node stop or start answering. */
  setQuiet(key: DeviceKey, quiet: boolean): void {
    if (quiet) this.quiet.add(key);
    else this.quiet.delete(key);
  }

  async open(): Promise<void> {
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
    const body = await this.hubAnswer(req.cmd, req.payload);
    const resp = encodeFrame({ flags: FLAG_RESP, seq: req.seq, cmd: req.cmd, payload: body });
    // Only the hub's own HELLO waits, so the checking state shows without slowing a scan.
    setTimeout(() => this.queue.put(resp), req.cmd === CMD.HELLO ? (this.opts.delayMs ?? 0) : 0);
  }

  read(timeoutMs: number): Promise<Uint8Array | null> {
    return this.queue.take(timeoutMs);
  }

  private async hubAnswer(cmd: number, p: Uint8Array): Promise<Uint8Array> {
    switch (cmd) {
      case CMD_RELAY:
        return this.relay(p);
      case CMD_XFER_BEGIN:
        this.topology = [];
        return Uint8Array.of(STATUS_OK);
      case CMD_XFER_DATA:
        this.topology.push(...p.subarray(4));
        return withStatus(STATUS_OK, u32(this.topology.length));
      case CMD_XFER_END: {
        const body = new Uint8Array(this.topology);
        this.received = body;
        const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', body));
        return withStatus(STATUS_OK, digest);
      }
      default:
        return answer(this.devices.get('hub')!, cmd);
    }
  }

  private relay(p: Uint8Array): Uint8Array {
    if (p.length < RELAY_HEAD_LEN) return Uint8Array.of(STATUS_ARG);
    const [link, addr] = p;
    const key: DeviceKey | null = link === LINK_I2C0 ? `i2c:${addr}` : link === LINK_PIO_UART ? 'uart' : null;
    const dev = key && !this.quiet.has(key) ? this.devices.get(key) : undefined;
    if (!dev) return Uint8Array.of(STATUS_TIMEOUT);
    const inner = parseFrame(p.subarray(RELAY_HEAD_LEN));
    const resp = encodeFrame({ flags: FLAG_RESP, seq: inner.seq, cmd: inner.cmd, payload: answer(dev, inner.cmd) });
    return withStatus(STATUS_OK, resp);
  }
}

function answer(dev: Device, cmd: number): Uint8Array {
  if (cmd === CMD.HELLO) return withStatus(STATUS_OK, encodeHelloResp(dev.hello));
  if (cmd === CMD.STATUS) return withStatus(STATUS_OK, encodeStatus(dev));
  return Uint8Array.of(STATUS_UNSUPPORTED);
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
  out.set([0, 0, 0, 0], tail);
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
