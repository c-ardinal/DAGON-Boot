import { CMD, FLAG_RESP, MAX_PAYLOAD_USB, STATUS_OK, encodeFrame, parseFrame } from './frame.ts';
import { ROLE_HUB, encodeHelloResp, versionRaw, type HelloResp } from './hello.ts';
import { FrameQueue, type Transport } from './transport.ts';

const STATUS_UNSUPPORTED = 0x02;

/** The sample hub: 1.0.0, confirmed, sample key id. */
export const SAMPLE_HUB_HELLO: HelloResp = {
  protoVer: 1,
  role: ROLE_HUB,
  portKind: 1,
  mcuId: 0,
  mode: 0,
  activeVersion: versionRaw(1, 0, 0),
  trial: false,
  keyId: Uint8Array.of(0x3f, 0x9a, 0x21, 0xc7, 0x00, 0x00, 0x00, 0x00),
  maxPayload: MAX_PAYLOAD_USB,
};

export interface MockOptions {
  hello?: HelloResp;
  /** Never answer (a device that is not a hub). */
  silent?: boolean;
  /** Reply delay, to make the checking state visible. */
  delayMs?: number;
}

/** In-memory hub that answers HELLO in the dagon-proto frame format. */
export class MockTransport implements Transport {
  private readonly queue = new FrameQueue();
  private isOpen = false;

  constructor(private readonly opts: MockOptions = {}) {}

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
    const body =
      req.cmd === CMD.HELLO
        ? withStatus(STATUS_OK, encodeHelloResp(this.opts.hello ?? SAMPLE_HUB_HELLO))
        : Uint8Array.of(STATUS_UNSUPPORTED);
    const resp = encodeFrame({ flags: FLAG_RESP, seq: req.seq, cmd: req.cmd, payload: body });
    setTimeout(() => this.queue.put(resp), this.opts.delayMs ?? 0);
  }

  read(timeoutMs: number): Promise<Uint8Array | null> {
    return this.queue.take(timeoutMs);
  }
}

function withStatus(status: number, body: Uint8Array): Uint8Array {
  const out = new Uint8Array(1 + body.length);
  out[0] = status;
  out.set(body, 1);
  return out;
}
