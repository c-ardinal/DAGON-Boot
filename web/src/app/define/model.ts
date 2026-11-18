// Define mode's document: the topology exactly as its YAML (field names and
// shapes of dagon-topology's schema) plus the page-side display names. Every
// edit is a pure function returning a new document, so undo keeps snapshots.
// Validity is never decided here: topo_check (dagon-topology in WASM) does it.
import { board_template, coordinator_capable, default_port, i2c_hzs, LinkKind, link_kind_table, rom_i2c_pairs, uart0_choices, link_kinds, max_nodes, rom_i2c_addrs, supported_mcus, topo_cut, topo_parse, uart_bauds } from '../../wasm/pkg/dagon_wasm.js';
import { defaultBoard, type Board } from '../../wasm/dagon.ts';
import { INSPECT, LINK_KIND_WORDS } from '../copy.ts';
import type { Config, Link as GLink, Node as GNode, Skip, UplinkHost, Wire } from '../sample.ts';

export type { UplinkHost } from '../sample.ts';
export { LinkKind } from '../../wasm/pkg/dagon_wasm.js';

/**
 * A link's or an uplink's kind as the page keeps it: its code (dagon-wasm's LinkKind), which
 * every decision reads. A name no kind has stays as the file wrote it, so the check names it.
 */
export type Kind = LinkKind | string;

/** The kind every bus has (dagon-topology's BusKind), not a link kind. */
const BUS_KIND = 'i2c';

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
  /** A 親機's hop towards the PC (or what drives it); none: USB CDC to dagon. */
  uplink?: { kind: Kind; host?: UplinkHost };
}

export interface TBus {
  id: string;
  kind: typeof BUS_KIND;
  controller: string;
  sda: number;
  scl: number;
  speed_hz?: number;
  pullups?: string;
}

export interface TControl {
  kind: 'full' | 'cooperative';
  rom_entry?: string;
}

/** One end of a link: the node, its `[[link_port]]`, and what the link uses on it. */
export interface TEnd {
  node: string;
  port: string;
  addr?: number;
  node_pins?: { sda: string; scl: string };
  pins?: { tx: number; rx: number };
  nrst?: number;
  boot?: number;
}

/** A link between two nodes: it has no direction. */
export interface TLink {
  kind: Kind;
  ends: [TEnd, TEnd];
  bus?: string;
  iface?: string;
  baud?: number;
  control?: TControl;
}

/** Links from a 親機 (0) outwards: how many links each node is from one. */
function hops(t: Topo): Map<string, number> {
  const out = new Map<string, number>();
  let at = t.nodes.filter((n) => n.role === 'coordinator').map((n) => n.id);
  at.forEach((id) => out.set(id, 0));
  for (let d = 1; at.length; d++) {
    const next: string[] = [];
    for (const l of t.links ?? []) {
      const [a, b] = l.ends.map((e) => e.node);
      for (const [x, y] of [[a, b], [b, a]]) if (at.includes(x) && !out.has(y)) (out.set(y, d), next.push(y));
    }
    at = next;
  }
  return out;
}

/**
 * The two ends of link `l` as the editor draws them: `up` the end nearer a 親機 (the first listed
 * on a tie), `down` the end the link is shown under. Display only: the link has no direction and
 * the check reads its ends unordered.
 */
export function sides(t: Topo, l: TLink): { up: TEnd; down: TEnd; upAt: 0 | 1; downAt: 0 | 1 } {
  const h = hops(t);
  const d = (e: TEnd) => h.get(e.node) ?? Infinity;
  const upAt: 0 | 1 = d(l.ends[1]) < d(l.ends[0]) ? 1 : 0;
  const downAt: 0 | 1 = upAt === 0 ? 1 : 0;
  return { up: l.ends[upAt], down: l.ends[downAt], upAt, downAt };
}

/** A link as the editor's fields read it: drawn `from` → `to`, the per-node fields from their end ([`setLink`] writes them back there). */
export type LinkView = TLink & Omit<TEnd, 'node' | 'port'> & { from: string; to: string };

export function linkView(t: Topo, l: TLink): LinkView {
  const { up, down } = sides(t, l);
  return { ...l, from: up.node, to: down.node, addr: down.addr, node_pins: down.node_pins, pins: up.pins, nrst: up.nrst, boot: up.boot };
}

/** The position of the link shown under node `id`, or -1. */
export function linkIndexOf(t: Topo, id: string): number {
  return (t.links ?? []).findIndex((l) => sides(t, l).down.node === id);
}

/** The selection key of link `l`: the node it is shown under. */
export const linkKey = (t: Topo, l: TLink): string => `link:${sides(t, l).down.node}`;

/** The end of a new link at node `n`, at the port its definitions name for `kind`. */
function endAt(n: TNode, kind: LinkKind, iface?: string): TEnd {
  return { node: n.id, port: default_port(n.mcu, n.board, kind, iface) ?? '' };
}

interface KindEntry {
  code: LinkKind;
  name: string;
}

let kindTable: KindEntry[] | undefined;

/** Every link kind with its 構成 name, in code order (dagon-wasm's link_kind_table, from dagon_format::with_link_kinds!). */
export const linkKindTable = (): KindEntry[] => (kindTable ??= link_kind_table() as KindEntry[]);

/** Every hop kind a 構成 may name, in the order the define screen offers them. */
export const linkKindCodes = (): LinkKind[] => linkKindTable().map((k) => k.code);

const nameOf = (k: Kind): string => (typeof k === 'number' ? (linkKindTable().find((e) => e.code === k)?.name ?? String(k)) : k);
const codeOf = (k: unknown): unknown => linkKindTable().find((e) => e.name === k)?.code ?? k;

/** How kind `k` reads on the canvas, in the table and in the file check. */
export const kindLabel = (k: Kind): string => LINK_KIND_WORDS[nameOf(k)]?.label ?? nameOf(k);

/** How kind `k` reads in the inspector's list of kinds. */
export const kindChoice = (k: Kind): string => LINK_KIND_WORDS[nameOf(k)]?.choice ?? nameOf(k);

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

/** `t` with each uplink's and link's kind as `to` gives it; every other value and every key's place stay. */
function withKinds(t: Topo, to: (k: unknown) => unknown): Topo {
  const kinded = <T,>(o: T): T => (isObject(o) && 'kind' in o ? ({ ...o, kind: to(o.kind) } as T) : o);
  const nodes = t.nodes.map((n) => (isObject(n) && isObject(n.uplink) ? { ...n, uplink: kinded(n.uplink) } : n));
  return { ...t, nodes, ...(Array.isArray(t.links) ? { links: t.links.map(kinded) } : {}) };
}

/** A 構成 as a file or dagon-wasm writes it (kinds by name), as the page keeps it (kinds by code). */
export const topoFromNames = (t: Topo): Topo => withKinds(t, codeOf);

/** The 構成 with its kinds by name, as its YAML writes them and the check reads them. */
export const topoWithNames = (t: Topo): Topo => withKinds(t, (k) => (typeof k === 'number' ? nameOf(k) : k));

/** Kinds `mcu`'s firmware implements up (to the PC or the 親機) and down (to 子機); the rest are not available. */
export function linkKinds(mcu: string): { up: LinkKind[]; down: LinkKind[] } {
  return link_kinds(mcu) as { up: LinkKind[]; down: LinkKind[] };
}

/** The I2C bus speeds every one of `nodes`' MCU definitions names (`[link_i2c] hzs`): the only ones a bus joining them may be set to. */
export function i2cHzs(nodes: (TNode | undefined)[]): number[] {
  return Array.from(i2c_hzs(nodes.map((n) => n?.mcu ?? '')));
}

/** The nodes on bus `id`: each end of every I2C link over it. */
export function busNodes(t: Topo, id: string): TNode[] {
  const ids = new Set((t.links ?? []).filter((l) => l.kind === LinkKind.I2c && l.bus === id).flatMap((l) => l.ends.map((e) => e.node)));
  return t.nodes.filter((n) => ids.has(n.id));
}

/** The UART speeds both ends' MCU definitions name (`[link_uart] bauds`): the only ones a link may be set to. */
export function uartBauds(a: TNode | undefined, b: TNode | undefined): number[] {
  return Array.from(uart_bauds(a?.mcu ?? '', b?.mcu ?? ''));
}

/** The pins a node's own end of a link may use, as the check (V-07) allows: its board's header, or, named by MCU alone, that MCU's GPIO range. */
export function definedPins(n: TNode | undefined): number[] {
  if (!n) return [];
  if (n.board) return (template(n.board)?.header ?? []).map((p) => Number(p.replace(/^GPIO/, ''))).filter((x) => Number.isFinite(x));
  const count = mcus().find((m) => m.id === n.mcu)?.gpioCount ?? 0;
  return Array.from({ length: count }, (_, i) => i);
}

/** The pins a hw-uart0 link from `n` may use: what V-09 and V-07 accept together, from the one function the check reads. */
export function uart0Pins(n: TNode | undefined): { tx: number[]; rx: number[] } {
  return n ? (uart0_choices(n.mcu, n.board) as { tx: number[]; rx: number[] }) : { tx: [], rx: [] };
}

/** The pin pairs `n`'s ROM loader listens on, as V-13 accepts. */
export function romI2cPairs(n: TNode | undefined): { sda: string; scl: string }[] {
  return n ? (rom_i2c_pairs(n.mcu) as { sda: string; scl: string }[]) : [];
}

/** The speed a new link from `h` to `n` starts at: the first both definitions name; none when they share none (the check then asks). */
const firstBaud = (h: TNode, n: TNode): { baud?: number } => {
  const b = uartBauds(h, n)[0];
  return b === undefined ? {} : { baud: b };
};

/** Whether a hop of `kind` from `from` to `to` is one both firmwares implement. */
export function hopAvailable(kind: Kind, from: TNode | undefined, to: TNode | undefined): boolean {
  return (!from || linkKinds(from.mcu).down.includes(kind as LinkKind)) && (!to || linkKinds(to.mcu).up.includes(kind as LinkKind));
}

/** Sets (or, with null, removes) 親機 `id`'s uplink. */
export function setUplink(doc: Doc, id: string, uplink: TNode['uplink'] | null): Doc {
  const nodes = doc.topo.nodes.map((n) => {
    if (n.id !== id) return n;
    const { uplink: _old, ...rest } = n;
    return uplink ? { ...rest, uplink } : rest;
  });
  return { ...doc, topo: { ...doc.topo, nodes } };
}

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
}

/** The board table's placement defaults (dagon-wasm's board_template). */
export interface BoardTemplate {
  mcu: string;
  port: string | null;
  flashMap: string | null;
  i2cTarget: boolean;
  header: string[];
  i2cPairs: { sda: string; scl: string }[];
  link: { kind: LinkKind; iface: string | null; addr: number | null; baud: number | null; control: 'full' | 'cooperative'; romEntry: string | null } | null;
  /** Pins of a new I2C bus / PIO UART link when the board is the 親機 (its [coordinator]). */
  coordinator: { i2c: { controller: string; sda: number; scl: number }; pioUart: { tx: number; rx: number } } | null;
}

/** The topology YAML number a new 構成 is written with (dagon-topology's SCHEMA_VERSION; model.test.ts checks it against the WASM's). */
export const SCHEMA_VERSION = 2;

/** Nodes the editor places and draws: the format's limit. */
export const maxNodes = (): number => max_nodes();
const I2C_FIRST = 0x08;
const I2C_LAST = 0x77;

export function template(board: string): BoardTemplate | null {
  return board_template(board) as BoardTemplate | null;
}

/** 親機 `h`'s board's [coordinator] pins (a node placed by MCU: its MCU's default board, if any). */
function hubPins(h: TNode): BoardTemplate['coordinator'] {
  const board = h.board ?? defaultBoard(h.mcu);
  return (board ? template(board)?.coordinator : null) ?? null;
}

/** A new I2C bus on 親機 `h` towards `n`: the first speed both definitions name (none: the check asks); without defined pins the check asks for them. */
function hubBus(h: TNode, n: TNode): TBus {
  const hz = i2cHzs([h, n])[0];
  return { id: 'i2c-main', kind: BUS_KIND, ...hubPins(h)?.i2c, ...(hz === undefined ? {} : { speed_hz: hz }), pullups: 'external' } as TBus;
}

/** UART0 pins for a link from `h`: its MCU's [coordinator] pair, else the 親機 MCU's; none defined: the check asks for them. */
function firstUart0(t: Topo, h: TNode): { pins?: { tx: number; rx: number } } {
  const c = [uart0Pins(h), uart0Pins(hub(t))].find((p) => p.tx.length && p.rx.length);
  return c ? { pins: { tx: c.tx[0], rx: c.rx[0] } } : {};
}

function hubUart(h: TNode): { tx: number; rx: number } {
  return { ...hubPins(h)?.pioUart } as { tx: number; rx: number };
}

/**
 * The parts of a 構成 the page fills in when a file leaves them out: one place, used when a
 * file is read, when a document starts empty, when a release's 構成 is taken in, and when two
 * documents are compared (a filled-in default is not a change).
 */
export function withDefaults(t: Topo): Topo {
  return { buses: [], links: [], recovery: {}, ...t, update: t.update ?? { order: [] } };
}

/** A document with every default filled in; any other key (a saved session's old `boards`) is dropped. */
export function docWithDefaults(doc: Doc): Doc {
  return { topo: withDefaults(doc.topo), names: doc.names };
}

export function emptyDoc(name = 'my-project'): Doc {
  return { topo: withDefaults({ dagon: SCHEMA_VERSION, name, nodes: [] } as unknown as Topo), names: {} };
}

/** A YAML file as a document; throws when it is not YAML at all. */
export function fromYaml(yaml: string, names: Record<string, string> = {}): Doc {
  const t = topo_parse(yaml) as Topo | null;
  // An empty file or one that is not a mapping with nodes is not a 構成, not a crash.
  if (!t || typeof t !== 'object' || Array.isArray(t) || !Array.isArray(t.nodes)) throw new Error('not a topology');
  return { topo: withDefaults(topoFromNames(t)), names };
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

/** Whether a board can be placed now: a known board and room left (the same board again is a second one of it). */
export function canPlace(doc: Doc, board: string): boolean {
  return template(board) !== null && doc.topo.nodes.length < maxNodes();
}

/** "置く": a board with no link of its own (it cannot be a 子機) is a 親機, the second and on too; else a 子機. */
export function place(doc: Doc, board: string, name?: string): { doc: Doc; id: string } {
  const tpl = template(board);
  if (!tpl) return { doc, id: '' };
  const t = doc.topo;
  const asHub = tpl.link === null;
  const id = asHub ? freeId(t, 'hub') : freeId(t, board.split('-')[0]);
  const node: TNode = {
    id,
    mcu: tpl.mcu,
    board,
    role: asHub ? 'coordinator' : 'node',
    port: tpl.port ?? '',
    ...(tpl.flashMap ? { flash_map: tpl.flashMap } : {}),
    max_attempts: asHub ? 3 : tpl.link?.kind === LinkKind.Uart ? 1 : 3,
    tag: id,
  };
  const recovery = { ...t.recovery, [id]: asHub ? ['local'] : ['local', 'redistribute'] };
  const topo = reorder({ ...t, nodes: [...t.nodes, node], recovery });
  return { doc: { topo, names: name ? { ...doc.names, [id]: name } : doc.names }, id };
}

/** "置く" by MCU: a node without a board; its flash is the MCU's own, from the MCU definition. */
export function placeMcu(doc: Doc, mcu: string): { doc: Doc; id: string } {
  const m = mcus().find((x) => x.id === mcu);
  if (!m || doc.topo.nodes.length >= maxNodes()) return { doc, id: '' };
  const t = doc.topo;
  const asHub = !hub(t) && coordinator_capable(mcu);
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
  return { doc: { ...doc, topo }, id };
}

/** The flash size the MCU definition's default flash map lays out, for a node that names no board (nothing is asked); null for a node with a board. */
export function ownFlash(doc: Doc, id: string): number | null {
  const n = doc.topo.nodes.find((x) => x.id === id);
  return n && !n.board ? (mcus().find((m) => m.id === n.mcu)?.flashBytes ?? null) : null;
}

/** I2C addresses no end of another link on `bus` uses, without the defined ROM loaders'. */
export function freeAddrs(t: Topo, except?: number): number[] {
  const used = new Set((t.links ?? []).filter((l, i) => l.kind === LinkKind.I2c && i !== except).flatMap((l) => l.ends.map((e) => e.addr)));
  const rom = rom_i2c_addrs();
  const out: number[] = [];
  for (let a = I2C_FIRST; a <= I2C_LAST; a++) if (!rom.includes(a) && !used.has(a)) out.push(a);
  return out;
}

/**
 * "つなぐ先": links node `id` to `parent` (the hub when omitted). From the
 * hub to a 子機 the board's default link (I2C or UART); between 親機 a link
 * of a kind both MCUs define a port for, and deeper a UART link, both
 * described only. Each end starts at the port its definitions name.
 */
export function connect(doc: Doc, id: string, parent?: string): Doc {
  const t = doc.topo;
  const h = parent ? t.nodes.find((x) => x.id === parent) : hub(t);
  const n = t.nodes.find((x) => x.id === id);
  if (!h || !n || n === h || linkIndexOf(t, id) >= 0) return doc;
  // A node chosen by MCU links like its MCU's default board, if any (its pins are the developer's).
  const stand = n.board ?? defaultBoard(n.mcu);
  const tpl = stand ? template(stand) : null;
  const d = tpl?.link ?? { kind: LinkKind.Uart, iface: 'pio-uart', addr: null, baud: null, control: 'cooperative' as const, romEntry: null };
  if (h.role !== 'coordinator' || n.role === 'coordinator') {
    // Below a 子機 the board's own way into its loader, as under the 親機 (V-11 asks for it on every UART link).
    const control: TControl = { kind: d.control, ...(d.romEntry ? { rom_entry: d.romEntry } : {}) };
    // Between 親機: CAN where both MCUs define a port for it, else the first kind both define one for.
    const both = (k: LinkKind) => [h, n].every((x) => default_port(x.mcu, x.board, k, undefined) != null);
    const kind = [LinkKind.Can, ...linkKindCodes().filter((k) => k !== LinkKind.I2c && k !== LinkKind.Uart)].find(both) ?? LinkKind.Can;
    const link: TLink =
      n.role === 'coordinator'
        ? { kind, ends: [endAt(h, kind), endAt(n, kind)] }
        : { kind: LinkKind.Uart, iface: 'hw-uart0', ends: [{ ...endAt(h, LinkKind.Uart, 'hw-uart0'), ...firstUart0(t, h) }, endAt(n, LinkKind.Uart, 'hw-uart0')], ...firstBaud(h, n), control };
    return { ...doc, topo: { ...t, links: [...(t.links ?? []), link] } };
  }
  let buses = t.buses ?? [];
  let link: TLink;
  if (d.kind === LinkKind.I2c) {
    if (!buses.length) buses = [hubBus(h, n)];
    const free = freeAddrs(t);
    const addr = d.addr !== null && free.includes(d.addr) ? d.addr : free[0];
    const pins = tpl?.i2cPairs[0];
    link = {
      kind: LinkKind.I2c,
      bus: buses[0].id,
      ends: [endAt(h, LinkKind.I2c), { ...endAt(n, LinkKind.I2c), addr, ...(pins ? { node_pins: { sda: pins.sda, scl: pins.scl } } : {}) }],
      control: { kind: d.control, ...(d.romEntry ? { rom_entry: d.romEntry } : {}) },
    };
  } else {
    const iface = d.iface ?? 'pio-uart';
    link = {
      kind: LinkKind.Uart,
      iface,
      ends: [{ ...endAt(h, LinkKind.Uart, iface), pins: hubUart(h) }, endAt(n, LinkKind.Uart, iface)],
      ...(d.baud !== null && uartBauds(h, n).includes(d.baud) ? { baud: d.baud } : firstBaud(h, n)),
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
    if (l.ends.some((e) => e.node === id)) next = removeLink(next, i);
  }
  const recovery = { ...next.topo.recovery };
  delete recovery[id];
  const names = { ...next.names };
  delete names[id];
  const topo = reorder({ ...next.topo, nodes: next.topo.nodes.filter((n) => n.id !== id), recovery });
  return { topo, names };
}

/** Changes one field of node `id`; a new id is carried into links, recovery, order and names. */
export function setNode(doc: Doc, id: string, field: keyof TNode, value: unknown): Doc {
  const t = doc.topo;
  const nodes = t.nodes.map((n) => (n.id === id ? withField(n, field, value) : n));
  if (field !== 'id' || typeof value !== 'string' || value === id) return { ...doc, topo: reorder({ ...t, nodes }) };
  const to = value;
  const swap = (x: string) => (x === id ? to : x);
  const links = (t.links ?? []).map((l) => ({ ...l, ends: l.ends.map((e) => ({ ...e, node: swap(e.node) })) as TLink['ends'] }));
  const recovery = Object.fromEntries(Object.entries(t.recovery ?? {}).map(([k, v]) => [swap(k), v]));
  const names = Object.fromEntries(Object.entries(doc.names).map(([k, v]) => [swap(k), v]));
  return { names, topo: { ...t, nodes, links, recovery, update: { ...t.update, order: t.update.order.map(swap) } } };
}

export function setName(doc: Doc, id: string, name: string): Doc {
  const names = { ...doc.names };
  if (name.trim()) names[id] = name.trim();
  else delete names[id];
  return { ...doc, names };
}

/**
 * Changes the field at `path` of link `index`; undefined removes it. The editor's per-node fields
 * land on their end: `addr` and `node_pins` on the end the link is shown under, `pins.tx`,
 * `pins.rx`, `control.nrst` and `control.boot` (its `nrst`, `boot`) on the other.
 */
export function setLink(doc: Doc, index: number, path: string, value: unknown): Doc {
  const t = doc.topo;
  const at = (l: TLink): string[] => {
    const [head, ...rest] = path.split('.');
    const s = sides(t, l);
    if (head === 'addr' || head === 'node_pins') return ['ends', String(s.downAt), head, ...rest];
    if (head === 'pins') return ['ends', String(s.upAt), head, ...rest];
    if (head === 'control' && (rest[0] === 'nrst' || rest[0] === 'boot')) return ['ends', String(s.upAt), rest[0]];
    return [head, ...rest];
  };
  const links = (t.links ?? []).map((l, i) => (i === index ? (withPath(l, at(l), value) as TLink) : l));
  return { ...doc, topo: { ...t, links } };
}

/** Switches link `index` to `kind` with that kind's defaults. */
export function setLinkKind(doc: Doc, index: number, kind: LinkKind): Doc {
  const l = (doc.topo.links ?? [])[index];
  if (!l || l.kind === kind) return doc;
  const node = (id: string) => doc.topo.nodes.find((x) => x.id === id);
  const { up, down } = sides(doc.topo, l);
  if (kind !== LinkKind.I2c && kind !== LinkKind.Uart) {
    const end = (e: TEnd): TEnd => { const n = node(e.node); return n ? endAt(n, kind) : { node: e.node, port: '' }; };
    const links = (doc.topo.links ?? []).map((x, i) => (i === index ? { kind, ends: [end(up), end(down)] as TLink['ends'] } : x));
    const buses = (doc.topo.buses ?? []).filter((b) => links.some((y) => y.bus === b.id));
    return { ...doc, topo: { ...doc.topo, links, buses } };
  }
  const without = removeLink(doc, index);
  const n = node(down.node);
  const saved = n?.board;
  // connect() takes the board's default kind; for the other kind give it a board-less default.
  const tpl = saved ? template(saved) : null;
  const t = without.topo;
  const h = t.nodes.find((x) => x.id === up.node) ?? hub(t)!;
  if (tpl?.link?.kind === kind && h.role === 'coordinator') return connect(without, down.node, h.id);
  const lower = n ? endAt(n, kind, kind === LinkKind.Uart ? 'pio-uart' : undefined) : { node: down.node, port: '' };
  const link: TLink =
    kind === LinkKind.Uart
      ? { kind, iface: 'pio-uart', ends: [{ ...endAt(h, kind, 'pio-uart'), pins: hubUart(h) }, lower], ...firstBaud(h, n ?? h), control: { kind: 'cooperative' } }
      : { kind, bus: t.buses?.[0]?.id ?? 'i2c-main', ends: [endAt(h, kind), { ...lower, addr: freeAddrs(t)[0] }], control: { kind: 'full' } };
  const buses = kind === LinkKind.I2c && !t.buses?.length ? [hubBus(h, n ?? h)] : t.buses;
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
  if (Array.isArray(o)) return o.map((x, i) => (String(i) === head ? (rest.length ? withPath(x, rest, value) : value) : x));
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

/** `{ k: v, ... }` on one line, nested maps the same way. */
function flow(o: Record<string, unknown>): string {
  const parts = Object.entries(o)
    .filter(([, v]) => v !== undefined && v !== null)
    .map(([k, v]) => `${k}: ${isScalar(v) ? scalar(k, v) : flow(v as Record<string, unknown>)}`);
  return `{ ${parts.join(', ')} }`;
}

function block(o: Record<string, unknown>, pad: string): string[] {
  const out: string[] = [];
  for (const [k, v] of Object.entries(o)) {
    if (v === undefined || v === null) continue;
    if (isScalar(v)) out.push(`${pad}${k}: ${scalar(k, v)}`);
    else if (Array.isArray(v)) {
      if (v.every(isScalar)) out.push(`${pad}${k}: [${v.map((x) => scalar(k, x)).join(', ')}]`);
      // A link's two ends: one line each.
      else if (k === 'ends') out.push(`${pad}${k}:`, ...v.map((e) => `${pad}  - ${flow(e as Record<string, unknown>)}`));
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
export function toYaml(topo: Topo): string {
  const t = topoWithNames(topo);
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
  // Depth by parent links from a 親機 nobody links to (0), at any depth; a 親機 linked from
  // another node sits below it, and a 子機 with no parent in the 子機 row.
  const shown = links.map((l) => sides(t, l));
  const parent = (id: string) => shown.find((s) => s.down.node === id)?.up.node;
  const depth = (id: string, seen = new Set<string>()): number => {
    const n = t.nodes.find((x) => x.id === id);
    const p = parent(id);
    if (!n || !p || seen.has(id)) return n?.role === 'coordinator' ? 0 : 1;
    seen.add(id);
    return depth(p, seen) + 1;
  };
  const yaml = toYaml(t);
  // One source for reach: the checked cut (none while the 構成 has errors, which 運用 waits for).
  const cut = topo_cut(yaml) as { reach: string[]; skipped: Record<string, Skip> } | null;
  const skipped = cut?.skipped ?? {};
  const mcuTable = mcus();
  const nodes: GNode[] = t.nodes.map((n, i) => {
    const b = boards.find((x) => x.id === n.board);
    // A node chosen by MCU (no board): the MCU's name and not its id.
    const mcuName = b?.mcuName ?? mcuTable.find((m) => m.id === n.mcu)?.name ?? n.mcu;
    const k = shown.findIndex((s) => s.down.node === n.id);
    const l = k >= 0 ? links[k] : undefined;
    const s = k >= 0 ? shown[k] : undefined;
    const via = !l ? '' : l.kind === LinkKind.I2c ? `${kindLabel(l.kind)} ${hex(s?.down.addr)}` : l.kind === LinkKind.Uart ? `${kindLabel(l.kind)} ${l.baud ?? ''}`.trim() : kindLabel(l.kind);
    return {
      id: n.id,
      no: i + 1,
      name: doc.names[n.id] ?? n.id,
      role: n.role === 'coordinator' ? 'hub' : 'node',
      product: b?.product ?? INSPECT.boardOwn,
      mcu: mcuName,
      mcuId: n.mcu,
      via,
      reset: gpio(s?.up.nrst),
      boot: gpio(s?.up.boot),
      depth: depth(n.id),
      ...(n.role === 'coordinator' && n.uplink && !l ? { uplink: { kind: n.uplink.kind, host: n.uplink.host ?? 'pc-cli' } } : {}),
      ...(skipped[n.id] ? { skip: skipped[n.id] } : {}),
    };
  });
  const glinks: GLink[] = links.map((l, k) => {
    const { up, down } = shown[k];
    const wires: Wire[] =
      l.kind !== LinkKind.I2c && l.kind !== LinkKind.Uart
        ? []
        : l.kind === LinkKind.I2c
        ? [
            { signal: 'SDA', hubPin: gpio(bus(l.bus)?.sda) ?? '', nodePin: down.node_pins?.sda ?? '', shared: true },
            { signal: 'SCL', hubPin: gpio(bus(l.bus)?.scl) ?? '', nodePin: down.node_pins?.scl ?? '', shared: true },
            { signal: 'GND', hubPin: 'GND', nodePin: 'GND', shared: true },
            ...(up.nrst !== undefined ? [{ signal: 'NRST', hubPin: gpio(up.nrst)!, nodePin: 'NRST', shared: false }] : []),
            ...(up.boot !== undefined ? [{ signal: 'BOOT0', hubPin: gpio(up.boot)!, nodePin: 'BOOT0', shared: false }] : []),
          ]
        : [
            { signal: 'TX → RX', hubPin: gpio(up.pins?.tx) ?? '', nodePin: '', shared: false },
            { signal: 'RX ← TX', hubPin: gpio(up.pins?.rx) ?? '', nodePin: '', shared: false },
            { signal: 'GND', hubPin: 'GND', nodePin: 'GND', shared: true },
          ];
    return { from: up.node, to: down.node, kind: l.kind, addr: hex(down.addr), wires };
  });
  return {
    name: t.name,
    yaml,
    reach: cut?.reach ?? [],
    nodes,
    links: glinks,
    order: t.update.order.filter((id) => t.nodes.some((n) => n.id === id)),
  };
}
