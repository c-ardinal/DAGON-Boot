import { decodeHelloResp, encodeHelloReq } from '../wasm/dagon.ts';
import { CMD, MAX_PAYLOAD_USB, STATUS_OK, encodeFrame, newFrameStream, parseFrame } from './frame.ts';
import { ROLE_HUB, type HelloResp } from './hello.ts';

/**
 * Why reopening the rebooted hub did not get it back (the diagnostic log
 * under the reconnect dialog). `away`: the port the page had would not
 * open; `none`: no other allowed device of the same kind appeared;
 * `several`: more than one appeared and no version to tell them by;
 * `otherHub`: a hub with another MCU or release key than the one connected;
 * `silent`: a candidate did not answer HELLO; `notHub`: it answered as a
 * node; `version`: a hub not on trial with the version just sent.
 */
export type ReconnectNote =
  | { kind: 'away' | 'none' | 'silent' | 'notHub' | 'otherHub' }
  | { kind: 'several'; count: number }
  | { kind: 'version'; version: number; trial: boolean };

/**
 * Frame-level link to the hub, the shape dagon-host drives (and what
 * dagon-wasm will receive from the page): whole frames in, whole frames out.
 * The page owns choosing and opening the device; protocol steps live in
 * dagon-host.
 */
/** What the link to the hub does while the hub reboots: goes away and is opened again, or stays. */
export type ReconnectKind = 'reopen' | 'stays';

export interface Transport {
  /** Largest frame payload of this link (USB CDC when absent). */
  maxPayload?(): number;
  /** What the link does while the hub reboots (reopen when absent). */
  reconnectKind?(): ReconnectKind;
  open(): Promise<void>;
  close(): Promise<void>;
  /** Sends one complete frame. */
  write(frame: Uint8Array): Promise<void>;
  /** Next complete, CRC-checked frame, or null when `timeoutMs` passes first. */
  read(timeoutMs: number): Promise<Uint8Array | null>;
  /**
   * The hub `first` reboots onto `version` on trial: reopening it later keeps
   * only that hub (notThisHub); null clears it.
   */
  expectHub?(version: number | null, first?: HubIdentity | null): void;
  /** Why the reopens since `expectHub` did not get the hub back, oldest first. */
  notes?(): ReconnectNote[];
}

/** What "the same 親機" means: HELLO carries no serial number, so the MCU and release key of the first HELLO. */
export type HubIdentity = Pick<HelloResp, 'mcuId' | 'keyId'>;

/**
 * The one check every path that adopts a port goes through (dagon-host's
 * reopen in L20, the device reported or chosen in L21, the CLI's hand-back):
 * why the device answering `h` is not the 親機 first seen as `first`, or null.
 * With `version` it must also be on trial with that version (the one just
 * sent); without it a hub back on its previous version still passes, for
 * CONFIRM to stop there (L22).
 */
export function notThisHub(h: HelloResp | null, first: HubIdentity | null, version: number | null): ReconnectNote | null {
  if (h === null) return { kind: 'silent' };
  if (h.role !== ROLE_HUB) return { kind: 'notHub' };
  if (first && (h.mcuId !== first.mcuId || h.keyId.join() !== first.keyId.join())) return { kind: 'otherHub' };
  if (version !== null && !(h.trial && h.activeVersion === version)) return { kind: 'version', version: h.activeVersion, trial: h.trial };
  return null;
}

const NOTE_KINDS: readonly string[] = ['away', 'none', 'silent', 'notHub', 'otherHub', 'several', 'version'];

/** A ReconnectNote (thrown where a candidate is turned down). */
export function isNote(e: unknown): e is ReconnectNote {
  return typeof e === 'object' && e !== null && NOTE_KINDS.includes((e as { kind?: string }).kind ?? '');
}

/** How long a candidate port has to answer HELLO. */
const PROBE_MS = 500;
/** Distinct reasons kept for the diagnostic log. */
const MAX_NOTES = 8;

/** HELLO on a port the page is not using yet; null when it does not open or answer as a DAGON device. */
export async function probeHello(port: SerialPort, waitMs = PROBE_MS): Promise<HelloResp | null> {
  const t = new WebSerialTransport(port, null);
  try {
    await t.open();
  } catch {
    return null;
  }
  try {
    const req = encodeHelloReq({ protoVer: 1, maxPayload: MAX_PAYLOAD_USB });
    await t.write(encodeFrame({ flags: 0, seq: 0, cmd: CMD.HELLO, payload: req }));
    const raw = await t.read(waitMs);
    const f = raw && parseFrame(raw);
    if (!f || f.cmd !== CMD.HELLO || f.payload[0] !== STATUS_OK) return null;
    return decodeHelloResp(f.payload.subarray(1));
  } catch {
    return null;
  } finally {
    await t.close().catch(() => undefined);
  }
}

/** Queue of received frames with timed waits; shared by the real and mock links. */
export class FrameQueue {
  private frames: Uint8Array[] = [];
  private waiter: ((f: Uint8Array | null) => void) | null = null;

  put(frame: Uint8Array): void {
    if (this.waiter) {
      const w = this.waiter;
      this.waiter = null;
      w(frame);
    } else {
      this.frames.push(frame);
    }
  }

  take(timeoutMs: number): Promise<Uint8Array | null> {
    const ready = this.frames.shift();
    if (ready) return Promise.resolve(ready);
    if (this.waiter) return Promise.reject(new Error('concurrent read'));
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.waiter = null;
        resolve(null);
      }, timeoutMs);
      this.waiter = (f) => {
        clearTimeout(timer);
        resolve(f);
      };
    });
  }

  clear(): void {
    this.frames = [];
    this.waiter?.(null);
    this.waiter = null;
  }
}

// USB CDC ignores the line rate, but `open` requires one.
const CDC_BAUD = 115200;

export class WebSerialTransport implements Transport {
  /** Web Serial reaches the hub's USB CDC port, which goes away on its reboot. */
  maxPayload(): number {
    return MAX_PAYLOAD_USB;
  }

  reconnectKind(): ReconnectKind {
    return 'reopen';
  }

  private readonly queue = new FrameQueue();
  private reader: ReadableStreamDefaultReader<Uint8Array> | null = null;
  private pump: Promise<void> | null = null;
  private closing = false;

  /** `serial` finds the device again when it came back as a new port (the hub after its reboot). */
  constructor(
    public port: SerialPort,
    private readonly serial: Pick<Serial, 'getPorts'> | null = globalThis.navigator?.serial ?? null,
  ) {}

  /** Allowed ports present when this one last opened: none of them is this device coming back. */
  private known = new Set<SerialPort>();
  /** Ports the browser reported connected since (navigator.serial 'connect'): candidates even when known. */
  private back = new Set<SerialPort>();
  /** Only a port that was open before may be looked for again; the first open is the one the user chose. */
  private opened = false;
  /** The version the hub reboots onto (expectHub); null when none was sent. */
  private expected: number | null = null;
  /** The hub as first seen (expectHub); null when not known. */
  private identity: HubIdentity | null = null;
  private log: ReconnectNote[] = [];

  async open(): Promise<void> {
    this.port = await this.reopenable();
    this.known = new Set((await this.serial?.getPorts()) ?? []);
    this.back.clear();
    this.opened = true;
    this.closing = false;
    this.pump = this.readLoop();
  }

  /** `version` the hub reboots onto; null once that is over (confirmed, stopped or given up). */
  expectHub(version: number | null, first: HubIdentity | null = null): void {
    this.expected = version;
    this.identity = version === null ? null : first;
    this.log = [];
  }

  notes(): ReconnectNote[] {
    return [...this.log];
  }

  /** navigator.serial 'connect': an allowed device came (back). */
  arrived(port: SerialPort): void {
    if (port !== this.port) this.back.add(port);
  }

  private note(n: ReconnectNote): void {
    // Each reason once: the reopen loop repeats the same ones every second.
    if (this.log.some((x) => JSON.stringify(x) === JSON.stringify(n))) return;
    this.log = [...this.log, n].slice(-MAX_NOTES);
  }

  /**
   * Opens this port, or else the allowed port of the same USB device
   * (vendor and product id) that appeared since: after a reboot the hub
   * enumerates again and the browser may hand it out as a new port, still
   * allowed when it reports the same USB serial number. Web Serial does not
   * show that serial number, so once a version was sent (`expectHub`) only
   * the hub on trial with it is kept, even when one port appeared (dagon-host
   * checks the version only); with none sent, a single port is taken as it
   * is. Otherwise nothing is opened
   * (another hub of the same kind must not be taken for this one) and the
   * user chooses again.
   */
  private async reopenable(): Promise<SerialPort> {
    try {
      await this.port.open({ baudRate: CDC_BAUD });
      return this.port;
    } catch (e) {
      if (!this.opened) throw e;
      this.note({ kind: 'away' });
      const pick = await this.candidate();
      if (!pick) throw e;
      await pick.open({ baudRate: CDC_BAUD });
      return pick;
    }
  }

  /** The one port that is this hub coming back, or null (with the reason noted). */
  private async candidate(): Promise<SerialPort | null> {
    const want = this.port.getInfo();
    const ports = (await this.serial?.getPorts()) ?? [];
    const same = ports.filter((p) => {
      const info = p.getInfo();
      return (!this.known.has(p) || this.back.has(p)) && p !== this.port && info.usbVendorId === want.usbVendorId && info.usbProductId === want.usbProductId;
    });
    // Without a version to tell by (no update ran), the one that appeared is taken as it is.
    if (same.length === 1 && this.expected === null) return same[0];
    if (same.length === 0 || this.expected === null) {
      this.note(same.length === 0 ? { kind: 'none' } : { kind: 'several', count: same.length });
      return null;
    }
    const mine: SerialPort[] = [];
    for (const p of same) {
      const h = await probeHello(p);
      const why = notThisHub(h, this.identity, this.expected);
      if (why) this.note(why);
      else mine.push(p);
    }
    if (mine.length > 1) this.note({ kind: 'several', count: mine.length });
    return mine.length === 1 ? mine[0] : null;
  }

  async close(): Promise<void> {
    this.closing = true;
    await this.reader?.cancel().catch(() => undefined);
    await this.pump;
    this.queue.clear();
    await this.port.close();
  }

  async write(frame: Uint8Array): Promise<void> {
    const w = this.port.writable?.getWriter();
    if (!w) throw new Error('port not open');
    try {
      await w.write(frame);
    } finally {
      w.releaseLock();
    }
  }

  read(timeoutMs: number): Promise<Uint8Array | null> {
    return this.queue.take(timeoutMs);
  }

  private async readLoop(): Promise<void> {
    const decoder = newFrameStream();
    try {
      await this.readStreams(decoder);
    } finally {
      decoder.free();
    }
  }

  private async readStreams(decoder: { push(chunk: Uint8Array): Uint8Array[] }): Promise<void> {
    // Non-fatal errors (break, framing, parity, buffer overrun) error the
    // current stream and the port then offers a fresh `readable`; fatal ones
    // (device lost) leave it null, which ends the loop.
    while (this.port.readable && !this.closing) {
      const reader = this.port.readable.getReader();
      this.reader = reader;
      try {
        for (;;) {
          const { value, done } = await reader.read();
          if (done) return;
          if (value) for (const f of decoder.push(value)) this.queue.put(f);
        }
      } catch {
        // Retry with the next readable, if any.
      } finally {
        reader.releaseLock();
        this.reader = null;
      }
    }
  }
}
