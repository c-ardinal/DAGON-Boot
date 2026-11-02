// The simulated desk (dagon-sim's SimHub through dagon-wasm's SimDesk) as a
// Transport and the clock dagon-host runs on: the same HostLink path as Web
// Serial, with the F401's ROM loader simulated. Present only in builds with
// the `sim` feature (tests and `npm run dev`); `simDesk()` is null otherwise.
import * as pkg from '../wasm/pkg/dagon_wasm.js';
import type { LinkClock } from './hostLink.ts';
import type { Transport } from './transport.ts';

interface Desk {
  write(bytes: Uint8Array): void;
  read(): Uint8Array;
  now(): number;
  advance(ms: number): void;
  /** Erases the I2C node at `addr` (only its ROM loader is left). */
  wipe(addr: number): boolean;
}

type DeskClass = new (seed: number) => Desk;

export interface SimLink {
  transport: Transport;
  clock: LinkClock;
  /** The desk, made on first use (dagon-wasm must be loaded by then). */
  desk: () => Desk;
}

/** A simulated desk, or null in builds without it. `prepare` runs once on the new desk. */
export function simDesk(seed = 1, prepare?: (d: Desk) => void): SimLink | null {
  const Desk = (pkg as unknown as Record<string, DeskClass | undefined>).SimDesk;
  if (!Desk) return null;
  let made: Desk | null = null;
  const desk = (): Desk => {
    if (!made) {
      made = new Desk(seed);
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
      desk().write(frame);
    },
    // The simulator answers inside write; whatever it said is handed out at once.
    async read() {
      const b = desk().read();
      return b.length ? b : null;
    },
  };
  const clock: LinkClock = {
    now: () => desk().now(),
    sleep: (ms) => {
      desk().advance(ms);
      return new Promise((r) => setTimeout(r, 0));
    },
  };
  return { transport, clock, desk };
}
