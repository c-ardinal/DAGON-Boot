// The in-memory PC-connected node reads every request with dagon-proto's reader before it
// acts: an unknown must field and a malformed payload are refused on the wire,
// for every command it serves, and nothing in the node connected to the PC changes.
import { describe, expect, test } from 'vitest';
import sampleYaml from '../../../topology/sample.yaml?raw';
import { encodeHelloReq, topo_targets, wireVersion } from '../wasm/dagon.ts';
import { CMD, STATUS, WIRE, encodeFrame, parseFrame } from './frame.ts';
import { MockTransport } from './mock.ts';

/** Field 300, must, no value: a field no command knows. */
const UNKNOWN_MUST = [0xd9, 0x04, 0x00];
/** Field number 0: never a field. */
const FIELD_ZERO = [0x01, 0x00];
/** What a refusal of field 300 carries after its status: field 32, the number as a varint. */
const NAMES_300 = [0x40, 0x02, 0xac, 0x02];

interface Case {
  name: string;
  cmd: () => number;
  /** Bytes before the place of the next field, and after it (the delimiter and bulk). */
  head: () => number[];
  tail?: number[];
}

const CASES: Case[] = [
  { name: 'HELLO', cmd: () => CMD.HELLO, head: () => [...encodeHelloReq({ protoVer: wireVersion(), maxPayload: WIRE.MAX_PAYLOAD_USB })] },
  { name: 'STATUS', cmd: () => CMD.STATUS, head: () => [] },
  // target APP, total_len 512; slot B; hdr_len 512
  { name: 'XFER_BEGIN', cmd: () => CMD.XFER_BEGIN, head: () => [0, 0, 2, 0, 0, 0x03, 1, 1, 0x05, 2, 0, 2] },
  { name: 'XFER_HDR', cmd: () => CMD.XFER_HDR, head: () => [0, 0, 0, 0], tail: [0, 1, 2] },
  { name: 'XFER_DATA', cmd: () => CMD.XFER_DATA, head: () => [0, 0, 0, 0], tail: [0, 1, 2] },
  { name: 'XFER_END', cmd: () => CMD.XFER_END, head: () => [] },
  { name: 'SET_PENDING', cmd: () => CMD.SET_PENDING, head: () => [0, 0x03, 1, 1] },
  { name: 'REBOOT', cmd: () => CMD.REBOOT, head: () => [] },
  { name: 'CONFIRM', cmd: () => CMD.CONFIRM, head: () => [] },
  { name: 'SHOW', cmd: () => CMD.SHOW, head: () => [1] },
  { name: 'RELAY', cmd: () => CMD.RELAY, head: () => [1], tail: [0, 9, 9] },
];

let seq = 0;

async function ask(t: MockTransport, cmd: number, payload: number[]): Promise<Uint8Array> {
  seq = (seq + 1) & 0xff;
  await t.write(encodeFrame({ flags: 0, seq, cmd, payload: Uint8Array.from(payload) }));
  const raw = await t.read(1000);
  const f = parseFrame(raw!);
  expect([f.seq, f.cmd]).toEqual([seq, cmd]);
  return f.payload;
}

/** What the node connected to the PC shows of itself: its STATUS and HELLO, and the reboots it took. */
async function state(t: MockTransport): Promise<unknown> {
  return { status: [...(await ask(t, CMD.STATUS, []))], hello: t.helloOf('hub'), reboots: [...t.reboots] };
}

describe('the in-memory PC-connected node refuses what the reader refuses', () => {
  for (const c of CASES) {
    test(`${c.name}: an unknown must field and a malformed payload change nothing`, async () => {
      const t = new MockTransport();
      await t.open();
      const before = await state(t);
      const must = await ask(t, c.cmd(), [...c.head(), ...UNKNOWN_MUST, ...(c.tail ?? [])]);
      expect([...must]).toEqual([STATUS.FIELD, ...NAMES_300]);
      const bad = await ask(t, c.cmd(), [...c.head(), ...FIELD_ZERO, ...(c.tail ?? [])]);
      expect([...bad]).toEqual([STATUS.ARG]);
      expect(await state(t)).toEqual(before);
      // No transfer was opened by a refused XFER_BEGIN or chunk.
      expect([...(await ask(t, CMD.XFER_END, []))]).toEqual([STATUS.STATE]);
    });
  }

  test('a cut XFER_BEGIN is E_ARG, not an error thrown', async () => {
    const t = new MockTransport();
    await t.open();
    expect([...(await ask(t, CMD.XFER_BEGIN, [0]))]).toEqual([STATUS.ARG]);
    expect([...(await ask(t, CMD.XFER_BEGIN, []))]).toEqual([STATUS.ARG]);
    expect([...(await ask(t, CMD.SET_PENDING, []))]).toEqual([STATUS.ARG]);
  });

  test('a relayed request is read for the node too', async () => {
    const t = new MockTransport();
    await t.open();
    const seg = topo_targets(sampleYaml).seg;
    if (seg.kind !== 'linked') throw new Error('the sample links seg');
    const inner = encodeFrame({ flags: 0, seq: 7, cmd: CMD.REBOOT, payload: Uint8Array.from(UNKNOWN_MUST) });
    // Core: the link; must field 1: the address; the delimiter; the inner frame.
    const outer = await ask(t, CMD.RELAY, [seg.link, 0x03, 1, seg.addr, 0, ...inner]);
    expect(outer[0]).toBe(STATUS.OK);
    const answer = parseFrame(outer.subarray(2));
    expect([answer.seq, answer.cmd, ...answer.payload]).toEqual([7, CMD.REBOOT, STATUS.FIELD, ...NAMES_300]);
    expect(t.reboots).toEqual([]);
  });
});
