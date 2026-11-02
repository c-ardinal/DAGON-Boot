import { newFrameStream } from './frame.ts';

/**
 * Frame-level link to the hub, the shape dagon-host drives (and what
 * dagon-wasm will receive from the page): whole frames in, whole frames out.
 * The page owns choosing and opening the device; protocol steps live in
 * dagon-host.
 */
export interface Transport {
  open(): Promise<void>;
  close(): Promise<void>;
  /** Sends one complete frame. */
  write(frame: Uint8Array): Promise<void>;
  /** Next complete, CRC-checked frame, or null when `timeoutMs` passes first. */
  read(timeoutMs: number): Promise<Uint8Array | null>;
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

  async open(): Promise<void> {
    this.port = await this.reopenable();
    this.known = new Set((await this.serial?.getPorts()) ?? []);
    this.closing = false;
    this.pump = this.readLoop();
  }

  /**
   * Opens this port, or else the one allowed port of the same USB device
   * (vendor and product id) that appeared since: after a reboot the hub
   * enumerates again and the browser may hand it out as a new port, still
   * allowed when it reports the same USB serial number. Web Serial does not
   * show that serial number, so with no or several such ports nothing is
   * opened (another hub of the same kind must not be taken for this one)
   * and the user chooses again.
   */
  private async reopenable(): Promise<SerialPort> {
    try {
      await this.port.open({ baudRate: CDC_BAUD });
      return this.port;
    } catch (e) {
      const want = this.port.getInfo();
      const ports = (await this.serial?.getPorts()) ?? [];
      const back = ports.filter((p) => {
        const info = p.getInfo();
        return !this.known.has(p) && p !== this.port && info.usbVendorId === want.usbVendorId && info.usbProductId === want.usbProductId;
      });
      if (back.length !== 1) throw e;
      await back[0].open({ baudRate: CDC_BAUD });
      return back[0];
    }
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
