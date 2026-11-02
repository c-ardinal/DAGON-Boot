// Define mode's document: the topology exactly as its YAML (field names and
// shapes of dagon-topology's schema) plus the page-side display names. Every
// edit is a pure function returning a new document, so undo keeps snapshots.
// Validity is never decided here: topo_check (dagon-topology in WASM) does it.
import { board_template, supported_mcus, topo_parse } from '../../wasm/pkg/dagon_wasm.js';
import { supportedBoards, type Board } from '../../wasm/dagon.ts';
import { INSPECT } from '../copy.ts';
import type { Config, Link as GLink, Node as GNode, Wire } from '../sample.ts';

export interface TNode {
  id: string;
  mcu: string;
  board?: string;
  role: 'coordinator' | 'node';
  port: string;
  flash_map?: string;
  max_attempts?: number;
  listen_window_ms?: number;
  tag: string;
}

export interface TBus {
  id: string;
  kind: 'i2c';
  controller: string;
  sda: number;
  scl: number;
  speed_hz?: number;
  pullups?: string;
}

export interface TControl {
  kind: 'full' | 'cooperative';
  nrst?: number;
  boot?: number;
  rom_entry?: string;
}

export interface TLink {
  from: string;
  to: string;
  kind: LinkKind;
  bus?: string;
  addr?: number;
  node_pins?: { sda: string; scl: string };
  iface?: string;
  pins?: { tx: number; rx: number };
  baud?: number;
  control?: TControl;
}

/** I2C and UART: 親機 to 子機, updatable. CAN and Ethernet: between 親機 and deeper, described only (V-23). */
export type LinkKind = 'i2c' | 'uart' | 'can' | 'ethernet';

/** The MCU table (dagon-wasm's supported_mcus): what choosing a node by MCU starts from. */
export interface McuInfo {
  id: string;
  name: string;
  port: string;
  flashMap: string | null;
  /** Flash the flash map needs; a board with less cannot hold it. */
  flashBytes: number | null;
  gpioCount: number;
}

export function mcus(): McuInfo[] {
  return supported_mcus() as McuInfo[];
}

/** What the developer fills for a node chosen by MCU (dagon.toml's [board.<id>]). */
export interface BoardFields {
  flashBytes?: number;
}

export interface Topo {
  dagon: number;
  name: string;
  nodes: TNode[];
  buses?: TBus[];
  links?: TLink[];
  recovery?: Record<string, string[]>;
  update: { order: string[] };
  constraints?: unknown[];
}

export interface Doc {
  topo: Topo;
  /** 呼び名 by node id (page-side, optional). */
  names: Record<string, string>;
  /** [board.<id>] of the nodes chosen by MCU. */
  boards?: Record<string, BoardFields>;
}

/** The board table's placement defaults (dagon-wasm's board_template). */
export interface BoardTemplate {
  mcu: string;
  port: string | null;
  flashMap: string | null;
  i2cTarget: boolean;
  header: string[];
  i2cPairs: { sda: string; scl: string }[];
  link: { kind: 'i2c' | 'uart'; iface: string | null; addr: number | null; baud: number | null; control: 'full' | 'cooperative'; romEntry: string | null } | null;
}

/** Nodes the editor places and draws; updating reaches 8 (V-23 says when more). */
export const MAX_NODES = 32;
/** The STM32 ROM loader's address: never offered (V-18). */
export const ROM_ADDR = 0x39;
const I2C_FIRST = 0x08;
const I2C_LAST = 0x77;
/** The hub's I2C pins and PIO UART pins (hackathon §4.1; V-17, V-09). */
const HUB_I2C = { sda: 16, scl: 17 };
const HUB_UART = { tx: 26, rx: 27 };
const I2C_SPEED = 400_000;

export function template(board: string): BoardTemplate | null {
  return board_template(board) as BoardTemplate | null;
}

export function emptyDoc(name = 'my-desk'): Doc {
  return { topo: { dagon: 1, name, nodes: [], buses: [], links: [], recovery: {}, update: { order: [] } }, names: {} };
}

/** A YAML file as a document; throws when it is not YAML at all. */
export function fromYaml(yaml: string, names: Record<string, string> = {}): Doc {
  const t = topo_parse(yaml) as Topo | null;
  // An empty file or one that is not a mapping with nodes is not a 構成 (C09), not a crash.
  if (!t || typeof t !== 'object' || Array.isArray(t) || !Array.isArray(t.nodes)) throw new Error('not a topology');
  return { topo: { buses: [], links: [], recovery: {}, ...t, update: t.update ?? { order: [] } }, names };
}

const hub = (t: Topo) => t.nodes.find((n) => n.role === 'coordinator');

/** Children by the current order, then the hub (V-14 wants the hub last). */
function reorder(t: Topo): Topo {
  const ids = t.nodes.map((n) => n.id);
  const kept = t.update.order.filter((id) => ids.includes(id));
  const all = [...kept, ...ids.filter((id) => !kept.includes(id))];
  const h = hub(t)?.id;
  return { ...t, update: { ...t.update, order: [...all.filter((id) => id !== h), ...(h ? [h] : [])] } };
}

function freeId(t: Topo, base: string): string {
  const clean = base.toLowerCase().replace(/[^a-z0-9-]/g, '').slice(0, 12) || 'node';
  let id = clean;
  for (let i = 2; t.nodes.some((n) => n.id === id); i++) id = `${clean}-${i}`;
  return id;
}

/** Whether a board can be placed now: room left and not a second hub-only board. */
export function canPlace(doc: Doc, board: string): boolean {
  const tpl = template(board);
  if (!tpl || doc.topo.nodes.length >= MAX_NODES) return false;
  return !doc.topo.nodes.some((n) => n.board === board);
}

/** "置く": the hub when there is none and the board can be one (no link of its own), else a child. */
export function place(doc: Doc, board: string, name?: string): { doc: Doc; id: string } {
  const tpl = template(board);
  if (!tpl) return { doc, id: '' };
  const t = doc.topo;
  const asHub = !hub(t) && tpl.link === null;
  const id = asHub ? freeId(t, 'hub') : freeId(t, board.split('-')[0]);
  const node: TNode = {
    id,
    mcu: tpl.mcu,
    board,
    role: asHub ? 'coordinator' : 'node',
    port: tpl.port ?? '',
    ...(tpl.flashMap ? { flash_map: tpl.flashMap } : {}),
    max_attempts: asHub ? 3 : tpl.link?.kind === 'uart' ? 1 : 3,
    tag: id,
  };
  const recovery = { ...t.recovery, [id]: asHub ? ['local'] : ['local', 'redistribute'] };
  const topo = reorder({ ...t, nodes: [...t.nodes, node], recovery });
  return { doc: { topo, names: name ? { ...doc.names, [id]: name } : doc.names }, id };
}

/** "置く" by MCU: a node without a board (P1); flash size and pins are the developer's to fill. */
export function placeMcu(doc: Doc, mcu: string): { doc: Doc; id: string } {
  const m = mcus().find((x) => x.id === mcu);
  if (!m || doc.topo.nodes.length >= MAX_NODES) return { doc, id: '' };
  const t = doc.topo;
  const asHub = !hub(t) && m.port === 'self-boot' && mcu === 'rp2040';
  const id = asHub ? freeId(t, 'hub') : freeId(t, mcu);
  const node: TNode = {
    id,
    mcu,
    role: asHub ? 'coordinator' : 'node',
    port: m.port,
    ...(m.flashMap ? { flash_map: m.flashMap } : {}),
    max_attempts: m.port === 'vendor-stage2' ? 1 : 3,
    tag: id,
  };
  const recovery = { ...t.recovery, [id]: asHub ? ['local'] : ['local', 'redistribute'] };
  const topo = reorder({ ...t, nodes: [...t.nodes, node], recovery });
  return { doc: { ...doc, topo, boards: { ...doc.boards, [id]: {} } }, id };
}

/** The board fields a node chosen by MCU still lacks (▲ ボード 未設定), or null when complete or not needed. */
export function boardMissing(doc: Doc, id: string): ('flash')[] | null {
  const n = doc.topo.nodes.find((x) => x.id === id);
  if (!n || n.board) return null;
  const b = doc.boards?.[id] ?? {};
  return b.flashBytes ? null : ['flash'];
}

/** Flash too small for the node's flash map: an error before building (§4.3). */
export function flashTooSmall(doc: Doc, id: string): number | null {
  const n = doc.topo.nodes.find((x) => x.id === id);
  const need = n ? mcus().find((m) => m.id === n.mcu)?.flashBytes : null;
  const have = doc.boards?.[id]?.flashBytes;
  return n && !n.board && need && have && have < need ? need : null;
}

export function setBoard(doc: Doc, id: string, fields: BoardFields): Doc {
  return { ...doc, boards: { ...doc.boards, [id]: { ...doc.boards?.[id], ...fields } } };
}

/** I2C addresses no other link on `bus` uses, without the ROM loader's. */
export function freeAddrs(t: Topo, except?: number): number[] {
  const used = new Set((t.links ?? []).filter((l, i) => l.kind === 'i2c' && i !== except).map((l) => l.addr));
  const out: number[] = [];
  for (let a = I2C_FIRST; a <= I2C_LAST; a++) if (a !== ROM_ADDR && !used.has(a)) out.push(a);
  return out;
}

/**
 * "つなぐ先": links node `id` to `parent` (the hub when omitted). From the
 * hub to a 子機 the board's default link (I2C or UART); between 親機 a CAN
 * link, and deeper a UART link, both described only (V-23).
 */
export function connect(doc: Doc, id: string, parent?: string): Doc {
  const t = doc.topo;
  const h = parent ? t.nodes.find((x) => x.id === parent) : hub(t);
  const n = t.nodes.find((x) => x.id === id);
  if (!h || !n || n === h || (t.links ?? []).some((l) => l.to === id)) return doc;
  if (h.role !== 'coordinator' || n.role === 'coordinator') {
    const kind: LinkKind = n.role === 'coordinator' ? 'can' : 'uart';
    const link: TLink = kind === 'can' ? { from: h.id, to: id, kind } : { from: h.id, to: id, kind, iface: 'hw-uart0', pins: { tx: 0, rx: 1 }, baud: 115200 };
    return { ...doc, topo: { ...t, links: [...(t.links ?? []), link] } };
  }
  // A node chosen by MCU links like the first listed board with that MCU (its pins are the developer's).
  const stand = n.board ?? supportedBoards().find((b) => b.mcu === n.mcu)?.id;
  const tpl = stand ? template(stand) : null;
  const d = tpl?.link ?? { kind: 'uart' as const, iface: 'pio-uart', addr: null, baud: 115200, control: 'cooperative' as const, romEntry: null };
  let buses = t.buses ?? [];
  let link: TLink;
  if (d.kind === 'i2c') {
    if (!buses.length) buses = [{ id: 'i2c-main', kind: 'i2c', controller: 'i2c0', ...HUB_I2C, speed_hz: I2C_SPEED, pullups: 'external' }];
    const free = freeAddrs(t);
    const addr = d.addr !== null && free.includes(d.addr) ? d.addr : free[0];
    const pins = tpl?.i2cPairs[0];
    link = {
      from: h.id,
      to: id,
      kind: 'i2c',
      bus: buses[0].id,
      addr,
      ...(pins ? { node_pins: { sda: pins.sda, scl: pins.scl } } : {}),
      control: { kind: d.control, ...(d.romEntry ? { rom_entry: d.romEntry } : {}) },
    };
  } else {
    link = {
      from: h.id,
      to: id,
      kind: 'uart',
      iface: d.iface ?? 'pio-uart',
      pins: { ...HUB_UART },
      baud: d.baud ?? 115200,
      control: { kind: d.control, ...(d.romEntry ? { rom_entry: d.romEntry } : {}) },
    };
  }
  return { ...doc, topo: { ...t, buses, links: [...(t.links ?? []), link] } };
}

export function removeLink(doc: Doc, index: number): Doc {
  const links = (doc.topo.links ?? []).filter((_, i) => i !== index);
  const buses = (doc.topo.buses ?? []).filter((b) => links.some((l) => l.bus === b.id));
  return { ...doc, topo: { ...doc.topo, links, buses } };
}

export function removeNode(doc: Doc, id: string): Doc {
  const t = doc.topo;
  let next: Doc = doc;
  for (let i = (t.links ?? []).length - 1; i >= 0; i--) {
    const l = (t.links ?? [])[i];
    if (l.from === id || l.to === id) next = removeLink(next, i);
  }
  const recovery = { ...next.topo.recovery };
  delete recovery[id];
  const names = { ...next.names };
  delete names[id];
  const boards = { ...next.boards };
  delete boards[id];
  const topo = reorder({ ...next.topo, nodes: next.topo.nodes.filter((n) => n.id !== id), recovery });
  return { topo, names, boards };
}

/** Changes one field of node `id`; a new id is carried into links, recovery, order and names. */
export function setNode(doc: Doc, id: string, field: keyof TNode, value: unknown): Doc {
  const t = doc.topo;
  const nodes = t.nodes.map((n) => (n.id === id ? withField(n, field, value) : n));
  if (field !== 'id' || typeof value !== 'string' || value === id) return { ...doc, topo: reorder({ ...t, nodes }) };
  const to = value;
  const swap = (x: string) => (x === id ? to : x);
  const links = (t.links ?? []).map((l) => ({ ...l, from: swap(l.from), to: swap(l.to) }));
  const recovery = Object.fromEntries(Object.entries(t.recovery ?? {}).map(([k, v]) => [swap(k), v]));
  const names = Object.fromEntries(Object.entries(doc.names).map(([k, v]) => [swap(k), v]));
  const boards = Object.fromEntries(Object.entries(doc.boards ?? {}).map(([k, v]) => [swap(k), v]));
  return { names, boards, topo: { ...t, nodes, links, recovery, update: { ...t.update, order: t.update.order.map(swap) } } };
}

export function setName(doc: Doc, id: string, name: string): Doc {
  const names = { ...doc.names };
  if (name.trim()) names[id] = name.trim();
  else delete names[id];
  return { ...doc, names };
}

/** Changes the field at `path` (`addr`, `control.nrst`, `pins.tx`, ...) of link `index`; undefined removes it. */
export function setLink(doc: Doc, index: number, path: string, value: unknown): Doc {
  const links = (doc.topo.links ?? []).map((l, i) => (i === index ? (withPath(l, path.split('.'), value) as TLink) : l));
  return { ...doc, topo: { ...doc.topo, links } };
}

/** Switches link `index` to `kind` with that kind's defaults. */
export function setLinkKind(doc: Doc, index: number, kind: LinkKind): Doc {
  const l = (doc.topo.links ?? [])[index];
  if (!l || l.kind === kind) return doc;
  if (kind === 'can' || kind === 'ethernet') {
    const links = (doc.topo.links ?? []).map((x, i) => (i === index ? { from: x.from, to: x.to, kind } : x));
    const buses = (doc.topo.buses ?? []).filter((b) => links.some((y) => y.bus === b.id));
    return { ...doc, topo: { ...doc.topo, links, buses } };
  }
  const without = removeLink(doc, index);
  const n = doc.topo.nodes.find((x) => x.id === l.to);
  const saved = n?.board;
  // connect() takes the board's default kind; for the other kind give it a board-less default.
  const tpl = saved ? template(saved) : null;
  const t = without.topo;
  const h = t.nodes.find((x) => x.id === l.from) ?? hub(t)!;
  if (tpl?.link?.kind === kind && h.role === 'coordinator') return connect(without, l.to, h.id);
  const link: TLink =
    kind === 'uart'
      ? { from: h.id, to: l.to, kind, iface: 'pio-uart', pins: { ...HUB_UART }, baud: 115200, control: { kind: 'cooperative' } }
      : { from: h.id, to: l.to, kind, bus: t.buses?.[0]?.id ?? 'i2c-main', addr: freeAddrs(t)[0], control: { kind: 'full' } };
  const buses = kind === 'i2c' && !t.buses?.length ? [{ id: 'i2c-main', kind: 'i2c' as const, controller: 'i2c0', ...HUB_I2C, speed_hz: I2C_SPEED, pullups: 'external' }] : t.buses;
  return { ...without, topo: { ...t, buses, links: [...(t.links ?? []), link] } };
}

export function setBus(doc: Doc, index: number, field: keyof TBus, value: unknown): Doc {
  const buses = (doc.topo.buses ?? []).map((b, i) => (i === index ? withField(b, field, value) : b));
  return { ...doc, topo: { ...doc.topo, buses } };
}

function withField<T extends object>(o: T, field: keyof T, value: unknown): T {
  const out = { ...o } as Record<string, unknown>;
  if (value === undefined || value === '') delete out[field as string];
  else out[field as string] = value;
  return out as T;
}

function withPath(o: object, path: string[], value: unknown): object {
  const [head, ...rest] = path;
  const cur = o as Record<string, unknown>;
  if (!rest.length) return withField(cur, head, value);
  return { ...cur, [head]: withPath((cur[head] as object) ?? {}, rest, value) };
}

// --- YAML out -------------------------------------------------------------

const HEX_KEYS = new Set(['addr']);
const PLAIN = /^[A-Za-z_][A-Za-z0-9_.\-/]*$/;

function scalar(key: string, v: unknown): string {
  if (typeof v === 'number') return HEX_KEYS.has(key) ? `0x${v.toString(16).padStart(2, '0')}` : String(v);
  if (typeof v === 'boolean') return String(v);
  const s = String(v);
  return PLAIN.test(s) && !/^(true|false|null|yes|no|on|off|~)$/i.test(s) ? s : JSON.stringify(s);
}

const isScalar = (v: unknown) => v === null || typeof v !== 'object';

function block(o: Record<string, unknown>, pad: string): string[] {
  const out: string[] = [];
  for (const [k, v] of Object.entries(o)) {
    if (v === undefined || v === null) continue;
    if (isScalar(v)) out.push(`${pad}${k}: ${scalar(k, v)}`);
    else if (Array.isArray(v)) {
      if (v.every(isScalar)) out.push(`${pad}${k}: [${v.map((x) => scalar(k, x)).join(', ')}]`);
      else {
        out.push(`${pad}${k}:`);
        for (const item of v) {
          const lines = block(item as Record<string, unknown>, `${pad}    `);
          out.push(`${pad}  - ${lines[0].trimStart()}`, ...lines.slice(1));
        }
      }
    } else if (Object.values(v).every(isScalar) && Object.keys(v).length <= 3 && k !== 'control') {
      out.push(`${pad}${k}: { ${Object.entries(v).map(([kk, vv]) => `${kk}: ${scalar(kk, vv)}`).join(', ')} }`);
    } else {
      out.push(`${pad}${k}:`, ...block(v as Record<string, unknown>, `${pad}  `));
    }
  }
  return out;
}

/** The topology as YAML in the repository's own layout ("書き出す"). */
export function toYaml(t: Topo): string {
  const top = { dagon: t.dagon, name: t.name, nodes: t.nodes, buses: t.buses?.length ? t.buses : undefined, links: t.links?.length ? t.links : undefined, recovery: t.recovery, update: t.update, constraints: t.constraints?.length ? t.constraints : undefined };
  const sections = Object.entries(top).filter(([, v]) => v !== undefined && !(typeof v === 'object' && v !== null && !Array.isArray(v) && !Object.keys(v).length));
  return sections.map(([k, v]) => block({ [k]: v }, '').join('\n')).join('\n\n').replace(/\n\n(?=(dagon|name):)/g, '\n') + '\n';
}

// --- the graph's view -------------------------------------------------------

const gpio = (n: number | undefined) => (n === undefined ? null : `GPIO${n}`);
const hex = (a: number | undefined) => (a === undefined ? '' : `0x${a.toString(16).padStart(2, '0')}`);

/** The document in the shape the graph and the operate session draw. */
export function toConfig(doc: Doc, boards: Board[]): Config {
  const t = doc.topo;
  const links = t.links ?? [];
  const bus = (id?: string) => (t.buses ?? []).find((b) => b.id === id);
  // Depth by parent links from the 親機 (0); nodes with no parent sit in the 子機 row.
  const parent = (id: string) => links.find((l) => l.to === id)?.from;
  const depth = (id: string, seen = new Set<string>()): number => {
    const n = t.nodes.find((x) => x.id === id);
    if (!n || n.role === 'coordinator') return 0;
    const p = parent(id);
    if (!p || seen.has(p)) return 1;
    seen.add(id);
    const pn = t.nodes.find((x) => x.id === p);
    return pn?.role === 'coordinator' ? 1 : depth(p, seen) + 1;
  };
  const mcuTable = mcus();
  const nodes: GNode[] = t.nodes.map((n, i) => {
    const b = boards.find((x) => x.id === n.board);
    // A node chosen by MCU (no board): 「自作 · RP2040」, the MCU's name and not its id.
    const mcuName = b?.mcuName ?? mcuTable.find((m) => m.id === n.mcu)?.name ?? n.mcu;
    const l = links.find((x) => x.to === n.id);
    const via = !l ? '' : l.kind === 'i2c' ? `I2C ${hex(l.addr)}` : l.kind === 'uart' ? `UART ${l.baud ?? ''}`.trim() : l.kind === 'can' ? 'CAN' : 'Ethernet';
    return {
      id: n.id,
      no: i + 1,
      name: doc.names[n.id] ?? n.id,
      role: n.role === 'coordinator' ? 'hub' : 'node',
      product: b?.product ?? INSPECT.boardOwn,
      mcu: mcuName,
      via,
      reset: gpio(l?.control?.nrst),
      boot: gpio(l?.control?.boot),
      target: n.role === 'coordinator' ? { kind: 'hub', addr: 0 } : l?.kind === 'i2c' ? { kind: 'i2c', addr: l.addr ?? 0 } : { kind: 'uart', addr: 0 },
      depth: depth(n.id),
    };
  });
  const glinks: GLink[] = links.map((l) => {
    const wires: Wire[] =
      l.kind === 'can' || l.kind === 'ethernet'
        ? []
        : l.kind === 'i2c'
        ? [
            { signal: 'SDA', hubPin: gpio(bus(l.bus)?.sda) ?? '', nodePin: l.node_pins?.sda ?? '', shared: true },
            { signal: 'SCL', hubPin: gpio(bus(l.bus)?.scl) ?? '', nodePin: l.node_pins?.scl ?? '', shared: true },
            { signal: 'GND', hubPin: 'GND', nodePin: 'GND', shared: true },
            ...(l.control?.nrst !== undefined ? [{ signal: 'NRST', hubPin: gpio(l.control.nrst)!, nodePin: 'NRST', shared: false }] : []),
            ...(l.control?.boot !== undefined ? [{ signal: 'BOOT0', hubPin: gpio(l.control.boot)!, nodePin: 'BOOT0', shared: false }] : []),
          ]
        : [
            { signal: 'TX → RX', hubPin: gpio(l.pins?.tx) ?? '', nodePin: '', shared: false },
            { signal: 'RX ← TX', hubPin: gpio(l.pins?.rx) ?? '', nodePin: '', shared: false },
            { signal: 'GND', hubPin: 'GND', nodePin: 'GND', shared: true },
          ];
    return { from: l.from, to: l.to, kind: l.kind === 'i2c' ? 'I2C' : l.kind === 'uart' ? 'UART' : l.kind === 'can' ? 'CAN' : 'Ethernet', addr: hex(l.addr), wires };
  });
  return { name: t.name, yaml: toYaml(t), nodes, links: glinks, order: t.update.order.filter((id) => t.nodes.some((n) => n.id === id)) };
}
