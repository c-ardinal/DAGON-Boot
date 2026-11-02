import { describe, expect, test } from 'vitest';
import { CMD, FLAG_RESP, STATUS_OK, encodeFrame, parseFrame } from './frame.ts';
import { ROLE_NODE, encodeHelloResp, versionRaw, type HelloResp } from './hello.ts';
import { SAMPLE_HUB_HELLO } from './mock.ts';
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

  describe('several devices of the same kind after the hub reboots', () => {
    const ids = { usbVendorId: 0x2e8a, usbProductId: 0x000a };
    const V100 = versionRaw(1, 0, 0);
    const V110 = versionRaw(1, 1, 0);

    /** A port that answers HELLO with `hello` (null: never answers), like a real hub over USB. */
    function answering(hello: HelloResp | null) {
      let ctrl: ReadableStreamDefaultController<Uint8Array> | null = null;
      const p = {
        readable: null as ReadableStream<Uint8Array> | null,
        writable: null as WritableStream<Uint8Array> | null,
        opens: 0,
        async open() {
          p.opens++;
          p.readable = new ReadableStream<Uint8Array>({ start: (c) => void (ctrl = c) });
          p.writable = new WritableStream<Uint8Array>({
            write(raw) {
              const req = parseFrame(raw);
              if (!hello) return;
              const body = Uint8Array.of(STATUS_OK, ...encodeHelloResp(hello));
              ctrl?.enqueue(encodeFrame({ flags: FLAG_RESP, seq: req.seq, cmd: req.cmd, payload: body }));
            },
          });
        },
        async close() {
          p.readable = null;
          p.writable = null;
        },
        getInfo: () => ids,
      };
      return p;
    }

    async function rebooted(after: ReturnType<typeof answering>[], sent?: number, first: HelloResp | null = null) {
      let lost = false;
      const mine = Object.assign(answering(null), {
        open: async () => {
          if (lost) throw new DOMException('lost', 'NetworkError');
        },
      });
      let ports: object[] = [mine];
      const serial = { getPorts: async () => ports as unknown as SerialPort[] };
      const t = new WebSerialTransport(mine as unknown as SerialPort, serial);
      await t.open();
      await t.close();
      if (sent !== undefined) t.expectHub(sent, first);
      lost = true;
      ports = [mine, ...after];
      return t;
    }

    const hub = (activeVersion: number, trial: boolean): HelloResp => ({ ...SAMPLE_HUB_HELLO, activeVersion, trial });

    test('keeps only the hub on trial with the version just sent, and notes why the others were not taken', async () => {
      const other = answering(hub(V100, false));
      const back = answering(hub(V110, true));
      const t = await rebooted([other, back], V110);
      await t.open();
      expect(t.port).toBe(back);
      expect(t.notes()).toEqual([{ kind: 'away' }, { kind: 'version', version: V100, trial: false }]);
      await t.close();
    });

    test('takes nothing when two hubs are on trial with that version, or when no version was sent', async () => {
      const t = await rebooted([answering(hub(V110, true)), answering(hub(V110, true))], V110);
      await expect(t.open()).rejects.toThrow('lost');
      expect(t.notes()).toContainEqual({ kind: 'several', count: 2 });

      const u = await rebooted([answering(hub(V110, true)), answering(hub(V110, true))]);
      await expect(u.open()).rejects.toThrow('lost');
      expect(u.notes()).toEqual([{ kind: 'away' }, { kind: 'several', count: 2 }]);
    });

    test('one device appeared after a version was sent: taken only when it is the hub on trial with it', async () => {
      const wrong = await rebooted([answering(hub(V100, false))], V110);
      await expect(wrong.open()).rejects.toThrow('lost');
      expect(wrong.notes()).toEqual([{ kind: 'away' }, { kind: 'version', version: V100, trial: false }]);

      const back = answering(hub(V110, true));
      const right = await rebooted([back], V110);
      await right.open();
      expect(right.port).toBe(back);
      await right.close();
    });

    test.each([
      ['another MCU', { mcuId: 9 }],
      ['another release key', { keyId: Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8) }],
    ])('L20: a hub of the same USB kind on trial with the version just sent but %s is not this 親機: closed, not kept, logged', async (_, differs) => {
      const first = hub(V100, false);
      const impostor = answering({ ...hub(V110, true), ...differs });
      const t = await rebooted([impostor], V110, first);
      await expect(t.open()).rejects.toThrow('lost');
      expect(t.notes()).toEqual([{ kind: 'away' }, { kind: 'otherHub' }]);
      expect([impostor.opens, impostor.readable]).toEqual([1, null]);

      const mine = answering(hub(V110, true));
      const u = await rebooted([impostor, mine], V110, first);
      await u.open();
      expect(u.port).toBe(mine);
      await u.close();
    });

    test('once the expected version is cleared, a single device that appeared is taken as it is again', async () => {
      const one = answering(hub(V100, false));
      const t = await rebooted([one], V110);
      t.expectHub(null);
      await t.open();
      expect(t.port).toBe(one);
      await t.close();
    });

    test('a candidate that does not answer, and none at all, are noted for the diagnostic log', async () => {
      const t = await rebooted([answering(null), answering({ ...hub(V110, true), role: ROLE_NODE })], V110);
      await expect(t.open()).rejects.toThrow('lost');
      expect(t.notes()).toEqual([{ kind: 'away' }, { kind: 'silent' }, { kind: 'notHub' }]);

      const u = await rebooted([], V110);
      await expect(u.open()).rejects.toThrow('lost');
      expect(u.notes()).toEqual([{ kind: 'away' }, { kind: 'none' }]);
    });

    test('a port the browser reported connected again is a candidate even though it was allowed before', async () => {
      const back = answering(hub(V110, true));
      const t = await rebooted([], V110);
      // `back` was already allowed when the hub was opened…
      (t as unknown as { known: Set<unknown> }).known.add(back);
      const ports = (await (t as unknown as { serial: Serial }).serial.getPorts()) as unknown[];
      ports.push(back);
      await expect(t.open()).rejects.toThrow('lost');
      // …and comes back through navigator.serial 'connect'.
      t.arrived(back as unknown as SerialPort);
      await t.open();
      expect(t.port).toBe(back);
      await t.close();
    });
  });

  test('ends when the port has no readable left (device lost)', async () => {
    const t = new WebSerialTransport(port([readable((c) => c.error(new DOMException('lost', 'NetworkError')))]));
    await t.open();
    expect(await t.read(50)).toBeNull();
    await t.close();
  });
});
