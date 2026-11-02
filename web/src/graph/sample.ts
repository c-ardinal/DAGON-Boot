// The sample desk set-up (hub + 7-segment + watch) used by the graph
// prototype. Positions are canvas pixels of the node's top-left corner.

export type LinkKind = 'I2C' | 'UART';

export interface SampleNode {
  id: string;
  no: number;
  name: string;
  role: '親機' | '子機';
  layer: 1 | 2;
  product: string;
  mcu: string;
  via: string;
  version: string;
  x: number;
  y: number;
}

export interface Wire {
  signal: string;
  shared: boolean;
}

export interface SampleLink {
  from: string;
  to: string;
  kind: LinkKind;
  wires: Wire[];
}

export const NODES: SampleNode[] = [
  { id: 'hub', no: 1, name: 'メイン', role: '親機', layer: 1, product: 'RP2040-Touch-LCD-1.28', mcu: 'RP2040', via: 'USB', version: '1.0.0', x: 280, y: 152 },
  { id: 'seg', no: 2, name: '7セグ', role: '子機', layer: 2, product: 'NUCLEO-F401RE', mcu: 'STM32F401RE', via: 'I2C 0x42', version: '1.0.0', x: 120, y: 520 },
  { id: 'watch', no: 3, name: 'ウォッチ', role: '子機', layer: 2, product: 'M5StopWatch', mcu: 'ESP32-S3', via: 'UART 115200', version: '1.0.0', x: 440, y: 520 },
];

export const LINKS: SampleLink[] = [
  {
    from: 'hub',
    to: 'seg',
    kind: 'I2C',
    wires: [
      { signal: 'SDA', shared: true },
      { signal: 'SCL', shared: true },
      { signal: 'GND', shared: true },
      { signal: 'NRST', shared: false },
      { signal: 'BOOT0', shared: false },
    ],
  },
  {
    from: 'hub',
    to: 'watch',
    kind: 'UART',
    wires: [
      { signal: 'TX', shared: false },
      { signal: 'RX', shared: false },
      { signal: 'GND', shared: true },
    ],
  },
];

/** Collapsed bundle label: "{kind} · {n} 本（うち共有 {m}）", brackets omitted when m = 0. */
export function bundleLabel(l: SampleLink): string {
  const shared = l.wires.filter((w) => w.shared).length;
  return `${l.kind} · ${l.wires.length} 本${shared ? `（うち共有 ${shared}）` : ''}`;
}
