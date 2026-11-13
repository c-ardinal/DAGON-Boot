// Reading a node's slots (STATUS) for what the detail offers.
import { statusRows, versionOlder, versionText, type StatusResp } from '../wasm/dagon.ts';
import { SLOT } from './copy.ts';

const SLOT_WRITTEN = 1;
const SLOT_CONFIRMED = 4;

/** Every STATUS entry a label names: what a node of an MCU the definitions do not name shows. */
const every = (labels: readonly string[]) => labels.map((_, i) => i);

/** The STATUS entries of the app of `mcu` (its slots and the factory slot), from its MCU definition. */
export function appRows(mcu: string | undefined): number[] {
  return (mcu && statusRows(mcu, 'app')) || every(SLOT.rows);
}

/** The STATUS entries of the app images of `mcu`: its app slots (A, then B when it has one). */
export function imageRows(mcu: string | undefined): number[] {
  const agent = (mcu && statusRows(mcu, 'agent')) || [every(SLOT.rows).length - 1];
  return appRows(mcu).filter((i) => !agent.includes(i));
}

/** The STATUS entries of the boot loader of `mcu`: its boot loader slots. */
export function coreRows(mcu: string | undefined): number[] {
  return (mcu && statusRows(mcu, 'bootcore')) || every(SLOT.coreRows);
}

/**
 * The previous version "前のバージョンに戻す" goes back to (UC-O10): another
 * of the node's image slots (`rows`) when it is confirmed, or written and
 * older than the running version (it was confirmed before the running one
 * took over). A newer written image is not a previous version; null when
 * there is none, as on a node with one slot.
 */
export function previousVersion(s: StatusResp | null | undefined, rows: number[]): string | null {
  if (!s || !rows.includes(s.active)) return null;
  const running = s.slots[s.active].version;
  for (const i of rows) {
    if (i === s.active) continue;
    const other = s.slots[i];
    const before = other.state === SLOT_CONFIRMED || (other.state === SLOT_WRITTEN && versionOlder(other.version, running));
    if (before) return versionText(other.version);
  }
  return null;
}
