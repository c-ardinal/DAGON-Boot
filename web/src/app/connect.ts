import { CMD, FLAG_RESP, MAX_PAYLOAD_USB, PROTO_VERSION, STATUS_OK, encodeFrame, parseFrame } from '../serial/frame.ts';
import { decodeHelloResp, encodeHelloReq, type HelloResp } from '../serial/hello.ts';
import { WebSerialTransport, type Transport } from '../serial/transport.ts';

// ponytail: keep equal to dagon-host's HELLO timeout once that crate defines it.
export const HELLO_TIMEOUT_MS = 1000;

/** Why connecting stopped; each maps to one bottom-band chip and sentence. */
export type ConnectFailure = 'not-found' | 'busy' | 'denied' | 'no-response';

export class ConnectError extends Error {
  constructor(readonly failure: ConnectFailure) {
    super(failure);
  }
}

/**
 * Web Serial rejections → failure kinds (WICG Web Serial):
 * requestPort with no choice → NotFoundError; blocked by policy or not
 * allowed → SecurityError; open on a port held elsewhere → NetworkError /
 * InvalidStateError.
 */
export function failureOf(err: unknown, phase: 'choose' | 'open'): ConnectFailure {
  const name = err instanceof DOMException ? err.name : '';
  if (name === 'SecurityError') return 'denied';
  if (phase === 'choose') return 'not-found';
  return 'busy';
}

/** Shows the browser's device chooser and opens the chosen port. */
export async function chooseAndOpen(serial: Serial): Promise<WebSerialTransport> {
  let port: SerialPort;
  try {
    port = await serial.requestPort();
  } catch (e) {
    throw new ConnectError(failureOf(e, 'choose'));
  }
  const t = new WebSerialTransport(port);
  try {
    await t.open();
  } catch (e) {
    throw new ConnectError(failureOf(e, 'open'));
  }
  return t;
}

/** Sends HELLO and waits for the matching response. */
export async function hello(t: Transport, seq = 1): Promise<HelloResp> {
  const req = encodeHelloReq({ protoVer: PROTO_VERSION, maxPayload: MAX_PAYLOAD_USB });
  await t.write(encodeFrame({ flags: 0, seq, cmd: CMD.HELLO, payload: req }));
  const deadline = Date.now() + HELLO_TIMEOUT_MS;
  for (;;) {
    const left = deadline - Date.now();
    const raw = left > 0 ? await t.read(left) : null;
    if (!raw) throw new ConnectError('no-response');
    const f = parseFrame(raw);
    if (!(f.flags & FLAG_RESP) || f.cmd !== CMD.HELLO || f.seq !== seq) continue;
    if (f.payload[0] !== STATUS_OK) throw new ConnectError('no-response');
    try {
      return decodeHelloResp(f.payload.subarray(1));
    } catch {
      throw new ConnectError('no-response');
    }
  }
}
