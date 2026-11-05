// The simulator (dagon-sim's SimHub through dagon-wasm's Simulator) as a
// Transport and the clock dagon-host runs on: the same HostLink path as Web
// Serial, with the F401's ROM loader simulated. Present only in builds with
// the `sim` feature (tests and `npm run dev`); `simulator()` is null otherwise.
import * as pkg from '../wasm/pkg/dagon_wasm.js';
import type { LinkClock } from './hostLink.ts';
import type { Transport } from './transport.ts';

interface SimHub {
  write(bytes: Uint8Array): void;
  read(): Uint8Array;
  now(): number;
  advance(ms: number): void;
  /** Erases the I2C node at `addr` (only its ROM loader is left). */
  wipe(addr: number): boolean;
}

type SimulatorClass = new (seed: number) => SimHub;

export interface SimLink {
  transport: Transport;
  clock: LinkClock;
  /** The simulated hub, made on first use (dagon-wasm must be loaded by then). */
  hub: () => SimHub;
}

/** A simulator, or null in builds without it. `prepare` runs once on the new hub. */
export function simulator(seed = 1, prepare?: (d: SimHub) => void): SimLink | null {
  const Make = (pkg as unknown as Record<string, SimulatorClass | undefined>).Simulator;
  if (!Make) return null;
  let made: SimHub | null = null;
  const hub = (): SimHub => {
    if (!made) {
      made = new Make(seed);
      prepare?.(made);
    }
    return made;
  };
  let open = false;
  const transport: Transport = {
    open: async () => void (open = true),
    close: async () => void (open = false),
    async write(frame) {
      if (!open) throw new Error('port not open');
      hub().write(frame);
    },
    // The simulator answers inside write; whatever it said is handed out at once.
    async read() {
      const b = hub().read();
      return b.length ? b : null;
    },
  };
  const clock: LinkClock = {
    now: () => hub().now(),
    sleep: (ms) => {
      hub().advance(ms);
      return new Promise((r) => setTimeout(r, 0));
    },
  };
  return { transport, clock, hub };
}
