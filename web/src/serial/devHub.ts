import { MockTransport, type DeviceKey } from './mock.ts';
import type { Transport } from './transport.ts';

/**
 * Stand-in hub picked by the page URL instead of Web Serial, or null for the
 * real device. `?mock` is the in-memory desk hub (`?mock=silent`: a device
 * that never answers; `&quiet=uart,i2c:66`: nodes that do not answer). A
 * simulated hub plugs in here as one more `Transport`, under its own
 * parameter.
 */
export function devTransport(q: URLSearchParams): Transport | null {
  if (!q.has('mock')) return null;
  const quiet = (q.get('quiet') ?? '').split(',').filter(Boolean) as DeviceKey[];
  return new MockTransport({ silent: q.get('mock') === 'silent', delayMs: 150, quiet });
}
