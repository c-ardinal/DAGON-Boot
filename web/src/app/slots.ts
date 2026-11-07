// Reading a node's slots (STATUS) for what the detail offers.
import { versionOlder, versionText, type StatusResp } from '../wasm/dagon.ts';

const SLOT_WRITTEN = 1;
const SLOT_CONFIRMED = 4;

/**
 * The previous version "前のバージョンに戻す" goes back to (UC-O10): the
 * other slot when it is confirmed, or written and older than the running
 * version (it was confirmed before the running one took over). A newer
 * written image is not a previous version; null when there is none.
 */
export function previousVersion(s: StatusResp | null | undefined): string | null {
  if (!s || s.active > 1) return null;
  const other = s.slots[1 - s.active];
  const running = s.slots[s.active].version;
  const before = other.state === SLOT_CONFIRMED || (other.state === SLOT_WRITTEN && versionOlder(other.version, running));
  return before ? versionText(other.version) : null;
}
