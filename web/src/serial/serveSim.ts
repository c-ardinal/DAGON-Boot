// The simulated desk of `dagon demo`: the hub's bytes go through dagon serve
// (POST /v1/sim/write, GET /v1/sim/read) instead of Web Serial, so dagon-host
// runs exactly as with a real hub. Its clock runs at the demo's speed, as the
// simulated desk's does.
import type { LinkClock } from './hostLink.ts';
import type { Transport } from './transport.ts';

/** What the link needs from the serve client. */
export interface SimCalls {
  simWrite(bytes: Uint8Array): Promise<void>;
  /** Bytes the simulated hub answered within `waitMs` (wall ms); empty when none. */
  simRead(waitMs: number): Promise<Uint8Array>;
}

export interface SimStand {
  transport: Transport;
  clock: LinkClock;
}

/** The demo's hub as a Transport and the clock at `speed` (simulated ms per wall ms). */
export function serveSim(calls: SimCalls, speed: number, wall: LinkClock = REAL): SimStand {
  let open = false;
  const transport: Transport = {
    open: async () => void (open = true),
    close: async () => void (open = false),
    async write(frame) {
      if (!open) throw new Error('port not open');
      await calls.simWrite(frame);
    },
    async read(timeoutMs) {
      if (!open) return null;
      const b = await calls.simRead(Math.max(1, Math.round(timeoutMs / speed)));
      return b.length ? b : null;
    },
  };
  const clock: LinkClock = {
    now: () => wall.now() * speed,
    sleep: (ms) => wall.sleep(ms / speed),
  };
  return { transport, clock };
}

const REAL: LinkClock = {
  now: () => performance.now(),
  sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
};
