// Tree layout of the 構成: no layer bands. Every node nobody
// links to starts a tree; each node sits centred over its subtree, at any
// depth, and the trees stand side by side. Rows take the tallest card at
// that depth, so rows line up across trees.

export interface LayoutNode {
  id: string;
  /** A 親機 roots its tree at depth 0; a 子機 nobody links to sits in the first row under them. */
  hub: boolean;
}

export interface LayoutOptions {
  /** Card width. */
  w: number;
  /** Measured card height by id (a default for cards not measured yet). */
  h: (id: string) => number;
  /** Horizontal gap between siblings; trees stand twice as far apart. */
  gapX: number;
  /** Vertical room between rows (where the links run). */
  gapY: number;
}

export interface Placed {
  x: number;
  y: number;
  depth: number;
}

export interface Layout {
  at: Record<string, Placed>;
  /** Width and height of everything placed, from (0, 0). */
  width: number;
  height: number;
  /** Ids of the trees' roots, left to right. */
  roots: string[];
}

export function treeLayout(nodes: readonly LayoutNode[], links: readonly { from: string; to: string }[], o: LayoutOptions): Layout {
  const ids = new Set(nodes.map((n) => n.id));
  const linked = new Set(links.filter((l) => ids.has(l.from)).map((l) => l.to));
  const roots = nodes.filter((n) => !linked.has(n.id));
  // Each node is placed once, under the first link that reaches it (a cycle cannot loop).
  const seen = new Set<string>();
  const kids = new Map<string, string[]>();
  const depth = new Map<string, number>();
  const walk = (id: string, d: number) => {
    seen.add(id);
    depth.set(id, d);
    const own = [...new Set(links.filter((l) => l.from === id && ids.has(l.to) && !seen.has(l.to)).map((l) => l.to))];
    own.forEach((c) => seen.add(c));
    kids.set(id, own);
    own.forEach((c) => walk(c, d + 1));
  };
  for (const r of roots) if (!seen.has(r.id)) walk(r.id, r.hub ? 0 : 1);
  // Nodes only reachable through a cycle: their own trees.
  for (const n of nodes) if (!seen.has(n.id)) (roots.push(n), walk(n.id, n.hub ? 0 : 1));

  const span = new Map<string, number>();
  const width = (id: string): number => {
    const k = kids.get(id) ?? [];
    const w = Math.max(o.w, k.reduce((a, c) => a + width(c) + o.gapX, -o.gapX));
    span.set(id, w);
    return w;
  };
  const rowH: number[] = [];
  for (const [id, d] of depth) rowH[d] = Math.max(rowH[d] ?? 0, o.h(id));
  const rowY: number[] = [];
  let y = 0;
  for (let d = 0; d < rowH.length; d++) {
    rowY[d] = y;
    y += (rowH[d] ?? 0) + (rowH[d] === undefined ? 0 : o.gapY);
  }

  const at: Record<string, Placed> = {};
  const put = (id: string, left: number) => {
    const w = span.get(id)!;
    const d = depth.get(id)!;
    at[id] = { x: left + (w - o.w) / 2, y: rowY[d], depth: d };
    const k = kids.get(id) ?? [];
    const kw = k.reduce((a, c) => a + span.get(c)! + o.gapX, -o.gapX);
    let x = left + (w - kw) / 2;
    for (const c of k) {
      put(c, x);
      x += span.get(c)! + o.gapX;
    }
  };
  let x = 0;
  for (const r of roots) {
    width(r.id);
    put(r.id, x);
    x += span.get(r.id)! + 2 * o.gapX;
  }
  const height = Math.max(0, ...Object.entries(at).map(([id, p]) => p.y + o.h(id)));
  return { at, width: Math.max(0, x - 2 * o.gapX), height, roots: roots.map((r) => r.id) };
}
