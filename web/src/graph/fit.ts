// The canvas's fit: the zoom and pan that put the whole graph in the free part of the window.

/** The zoom's limits and one step of it. */
export const ZOOM_LIMITS = { min: 0.5, max: 2, step: 1.2 };
/** Room around the graph when fitting, and between a card brought in by focus and the box's edge (px). */
export const SIDE = 24;

export const clampZoom = (z: number): number => Math.min(ZOOM_LIMITS.max, Math.max(ZOOM_LIMITS.min, z));

/** A rectangle in canvas px. */
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Everything placed: each card, with `top` px kept above it (a top coordinator's uplink). */
export function boundsOf(cards: readonly { x: number; y: number; w: number; h: number; top: number }[]): Box {
  const x = Math.min(...cards.map((c) => c.x));
  const y = Math.min(...cards.map((c) => c.y - c.top));
  return { x, y, w: Math.max(...cards.map((c) => c.x + c.w)) - x, h: Math.max(...cards.map((c) => c.y + c.h)) - y };
}

/** The zoom at which `b` fills `free`, at most `cap`. */
const zoomFor = (b: Box, free: DOMRect, cap: number) => clampZoom(Math.min(cap, free.width / (b.w + 2 * SIDE), free.height / (b.h + 2 * SIDE)));

export interface FitInput {
  /** The graph's bounds with normal cards, and with compact ones. */
  normal: Box;
  compact: () => Box;
  /** The box the canvas is in, the free part of the window, and the whole area (tried when the free part alone would make the cards compact). */
  box: DOMRect;
  free: DOMRect;
  whole?: DOMRect;
  /** The largest zoom the fit takes. */
  cap: number;
  /** The zoom under which the cards turn compact. */
  compactZoom: number;
  /** So many nodes that the cards are compact at any zoom. */
  many: boolean;
}

/** The explicit fit: the same answer each time, whatever the cards were. */
export function fitView({ normal, compact, box, free, whole, cap, compactZoom, many }: FitInput): { zoom: number; x: number; y: number } {
  // The normal cards' bounds decide; when they would not fit at a zoom that keeps them normal, the compact layout is fitted and the zoom is held under the compact threshold so the result stays compact.
  let b = normal;
  let zoom = many ? 0 : zoomFor(b, free, cap);
  // The room kept for a toast never turns the cards compact: the whole area is used then, and a toast stands over the fit.
  if (!many && zoom < compactZoom && whole && zoomFor(b, whole, cap) >= compactZoom) {
    free = whole;
    zoom = zoomFor(b, whole, cap);
  }
  if (zoom < compactZoom) {
    b = compact();
    zoom = Math.min(zoomFor(b, free, cap), many ? cap : compactZoom * 0.99);
  }
  return {
    zoom,
    x: free.left - box.left + (free.width - b.w * zoom) / 2 - b.x * zoom,
    y: free.top - box.top + (free.height - b.h * zoom) / 2 - b.y * zoom,
  };
}
