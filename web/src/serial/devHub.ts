import type { LinkClock } from './hostLink.ts';
import { MockTransport, type DeviceKey } from './mock.ts';
import { simDesk } from './simHub.ts';
import type { Transport } from './transport.ts';

/** A stand-in hub, the clock dagon-host runs on with it, and the files the public site would hold. */
export interface DevHub {
  transport: Transport;
  clock?: LinkClock;
  fetchAsset?: (path: string) => Promise<Uint8Array | null>;
}

/** The repository's test-key release and F401 boot core, standing in for the public site. */
async function fixtureAsset(path: string): Promise<Uint8Array | null> {
  // Keeps the test-key files out of production builds.
  if (!import.meta.env.DEV) return null;
  const url =
    path === 'release/latest.dgb'
      ? (await import('../../../crates/dagon-sim/fixtures/desk-1.1.0.dgb?url')).default
      : path === 'release/seg-bootcore.img'
        ? (await import('../../../crates/dagon-wasm/fixtures/f401-bootcore-1.0.0.img?url')).default
        : null;
  if (!url) return null;
  const r = await fetch(url);
  return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
}

/**
 * Stand-in hub picked by the page URL instead of Web Serial, or null for the
 * real device (development builds only).
 * - `?mock`: the in-memory desk hub (`?mock=silent`: a device that never
 *   answers; `&quiet=uart,i2c:66`: nodes that do not answer; `&away`: after
 *   its REBOOT the hub stays away until chosen again; `&factory=uart`: nodes
 *   answering as their factory agent).
 * - `?sim`: the simulated desk (dagon-sim through dagon-wasm, ROM loader
 *   included) on its own clock; `&wipe` erases the F401 first. Both take the
 *   repository's test-key release and boot core as the public site's files.
 */
export function devHub(q: URLSearchParams): DevHub | null {
  if (q.has('sim')) {
    const s = simDesk(1, (d) => q.has('wipe') && d.wipe(0x42));
    if (!s) return null;
    return { transport: s.transport, clock: s.clock, fetchAsset: fixtureAsset };
  }
  if (!q.has('mock')) return null;
  const keys = (k: string) => (q.get(k) ?? '').split(',').filter(Boolean) as DeviceKey[];
  const transport = new MockTransport({
    silent: q.get('mock') === 'silent',
    delayMs: 150,
    quiet: keys('quiet'),
    factory: keys('factory'),
    hubAway: q.has('away') ? Infinity : undefined,
  });
  return { transport, fetchAsset: fixtureAsset };
}
