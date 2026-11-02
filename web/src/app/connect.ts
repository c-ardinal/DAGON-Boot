import { dagonReady } from '../wasm/dagon.ts';
import { WebSerialTransport } from '../serial/transport.ts';
import type { ConnectFailure } from './machine.ts';

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

/** Waits for dagon-wasm; false when it could not load. */
export async function wasmReady(ready: () => Promise<unknown> = dagonReady): Promise<boolean> {
  try {
    await ready();
    return true;
  } catch {
    return false;
  }
}
