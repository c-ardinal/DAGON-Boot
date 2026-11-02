import { describe, expect, test } from 'vitest';
import { CMD, encodeFrame } from './frame.ts';
import { WebSerialTransport } from './transport.ts';

interface FakeReadable {
  used: boolean;
  readonly locked: boolean;
  getReader(): ReadableStreamDefaultReader<Uint8Array>;
}

function readable(start: UnderlyingDefaultSource<Uint8Array>['start']): FakeReadable {
  const s = new ReadableStream<Uint8Array>({ start });
  const f: FakeReadable = {
    used: false,
    get locked() {
      return s.locked;
    },
    getReader() {
      f.used = true;
      return s.getReader();
    },
  };
  return f;
}

/** Port that, like Web Serial, offers a fresh `readable` after a non-fatal error. */
class FakePort extends EventTarget {
  private k = 0;
  writable = null;
  constructor(private readonly streams: FakeReadable[]) {
    super();
  }
  get readable(): FakeReadable | null {
    while (this.streams[this.k]?.used && !this.streams[this.k].locked) this.k++;
    return this.streams[this.k] ?? null;
  }
  async open() {}
  async close() {}
  getInfo() {
    return {};
  }
}

const port = (streams: FakeReadable[]) => new FakePort(streams) as unknown as SerialPort;

describe('WebSerialTransport read loop', () => {
  const frame = encodeFrame({ flags: 1, seq: 1, cmd: CMD.HELLO, payload: Uint8Array.of(0) });

  test('keeps reading on the next readable after a framing error', async () => {
    const t = new WebSerialTransport(
      port([readable((c) => c.error(new DOMException('framing', 'FramingError'))), readable((c) => c.enqueue(frame))]),
    );
    await t.open();
    expect(await t.read(500)).toEqual(frame);
    await t.close();
  });

  test('ends when the port has no readable left (device lost)', async () => {
    const t = new WebSerialTransport(port([readable((c) => c.error(new DOMException('lost', 'NetworkError')))]));
    await t.open();
    expect(await t.read(50)).toBeNull();
    await t.close();
  });
});
