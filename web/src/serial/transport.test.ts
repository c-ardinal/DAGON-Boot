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

  test('after the device left, opens the same USB device that came back as another allowed port, and reads again', async () => {
    const ids = { usbVendorId: 0x2e8a, usbProductId: 0x000a };
    let lost = false;
    const gone = Object.assign(new FakePort([readable(() => undefined)]), {
      open: () => (lost ? Promise.reject(new DOMException('lost', 'NetworkError')) : Promise.resolve()),
      getInfo: () => ids,
    });
    const other = Object.assign(new FakePort([readable(() => undefined)]), { getInfo: () => ({ usbVendorId: 1 }) });
    const back = Object.assign(new FakePort([readable((c) => c.enqueue(frame))]), { getInfo: () => ids });
    let ports: FakePort[] = [gone, other];
    const serial = { getPorts: async () => ports as unknown as SerialPort[] };
    const t = new WebSerialTransport(gone as unknown as SerialPort, serial);
    await t.open();
    await t.close();
    lost = true;
    ports = [gone, other, back];
    await t.open();
    expect(t.port).toBe(back);
    expect(await t.read(500)).toEqual(frame);
    await t.close();
  });

  test('the first open is only the port the user chose, even when another of the same kind is allowed', async () => {
    const ids = { usbVendorId: 0x2e8a, usbProductId: 0x000a };
    const chosen = Object.assign(new FakePort([]), {
      open: () => Promise.reject(new DOMException('busy', 'NetworkError')),
      getInfo: () => ids,
    });
    const sibling = Object.assign(new FakePort([readable(() => undefined)]), { getInfo: () => ids });
    const serial = { getPorts: async () => [chosen, sibling] as unknown as SerialPort[] };
    const t = new WebSerialTransport(chosen as unknown as SerialPort, serial);
    await expect(t.open()).rejects.toThrow('busy');
    expect(t.port).toBe(chosen);
  });

  test('never takes another hub of the same kind: not one that was there before, not one of two that appeared', async () => {
    const ids = { usbVendorId: 0x2e8a, usbProductId: 0x000a };
    const same = () => Object.assign(new FakePort([readable(() => undefined)]), { getInfo: () => ids });
    let lost = false;
    const mine = Object.assign(new FakePort([readable(() => undefined)]), {
      open: () => (lost ? Promise.reject(new DOMException('lost', 'NetworkError')) : Promise.resolve()),
      getInfo: () => ids,
    });
    const sibling = same();
    let ports: FakePort[] = [mine, sibling];
    const serial = { getPorts: async () => ports as unknown as SerialPort[] };
    const t = new WebSerialTransport(mine as unknown as SerialPort, serial);
    await t.open();
    await t.close();
    lost = true;
    await expect(t.open()).rejects.toThrow('lost');

    ports = [mine, sibling, same(), same()];
    await expect(t.open()).rejects.toThrow('lost');
    expect(t.port).toBe(mine);
  });

  test('ends when the port has no readable left (device lost)', async () => {
    const t = new WebSerialTransport(port([readable((c) => c.error(new DOMException('lost', 'NetworkError')))]));
    await t.open();
    expect(await t.read(50)).toBeNull();
    await t.close();
  });
});
