// The sample desk set-up (desk-demo): hub, 7-segment node on I2C and watch on
// UART, with the names the page uses for them and the physical wiring of each
// link. The YAML is the repository's sample topology; it is what "send to the
// hub" compiles and sends.
import deskDemoYaml from '../../../topology/desk-demo.yaml?raw';

export const DESK_DEMO_YAML: string = deskDemoYaml;

export type LinkKind = 'USB' | 'UART' | 'I2C' | 'BLE' | 'Wi-Fi' | 'Ethernet' | 'CAN';

/** How dagon-host reaches a node: the hub itself, or RELAY on I2C / UART. */
export interface Target {
  kind: 'hub' | 'i2c' | 'uart';
  addr: number;
}

export interface Node {
  id: string;
  /** Number shown as "[2]". */
  no: number;
  /** Name on screen. */
  name: string;
  role: 'hub' | 'node';
  product: string;
  mcu: string;
  /** Link value shown on the node: "I2C 0x42", "UART 115200"; empty for the hub. */
  via: string;
  /** Hub pins driving reset and boot; null when the node switches itself. */
  reset: string | null;
  boot: string | null;
  target: Target;
  /** Row on the graph: 0 親機, 1 子機, 2 and on deeper. Absent: by role. */
  depth?: number;
}

export interface Wire {
  signal: string;
  hubPin: string;
  nodePin: string;
  shared: boolean;
}

export interface Link {
  from: string;
  to: string;
  kind: LinkKind;
  /** Address value for the spoken name ("0x42"), empty for UART. */
  addr: string;
  wires: Wire[];
}

export interface Config {
  name: string;
  yaml: string;
  nodes: Node[];
  links: Link[];
  /** Node ids, children first and the hub last. */
  order: string[];
  /** The 親機's declared uplink (none: USB to this PC). */
  uplink?: { kind: string; host: 'pc-cli' | 'browser' | 'embedded' };
}

const NRST_HEADER = 'NRST（ヘッダの位置: 未設定）';
const BOOT0_HEADER = 'BOOT0（ヘッダの位置: 未設定）';

export const DESK_DEMO: Config = {
  name: 'desk-demo',
  yaml: DESK_DEMO_YAML,
  nodes: [
    { id: 'hub', no: 1, name: 'メイン', role: 'hub', product: 'Waveshare RP2040-Touch-LCD-1.28', mcu: 'RP2040', via: '', reset: null, boot: null, target: { kind: 'hub', addr: 0 } },
    { id: 'seg', no: 2, name: '7セグ', role: 'node', product: 'NUCLEO-F401RE', mcu: 'STM32F401RE', via: 'I2C 0x42', reset: 'GPIO18', boot: 'GPIO28', target: { kind: 'i2c', addr: 0x42 } },
    { id: 'watch', no: 3, name: 'ウォッチ', role: 'node', product: 'M5StopWatch', mcu: 'ESP32-S3', via: 'UART 115200', reset: null, boot: null, target: { kind: 'uart', addr: 0 } },
  ],
  links: [
    {
      from: 'hub',
      to: 'seg',
      kind: 'I2C',
      addr: '0x42',
      wires: [
        { signal: 'SDA', hubPin: 'GPIO16', nodePin: 'PB4 · D5', shared: true },
        { signal: 'SCL', hubPin: 'GPIO17', nodePin: 'PA8 · D7', shared: true },
        { signal: 'GND', hubPin: 'GND', nodePin: 'GND', shared: true },
        { signal: 'NRST', hubPin: 'GPIO18', nodePin: NRST_HEADER, shared: false },
        { signal: 'BOOT0', hubPin: 'GPIO28', nodePin: BOOT0_HEADER, shared: false },
      ],
    },
    {
      from: 'hub',
      to: 'watch',
      kind: 'UART',
      addr: '',
      wires: [
        { signal: 'TX → RX', hubPin: 'GPIO26', nodePin: 'G44', shared: false },
        { signal: 'RX ← TX', hubPin: 'GPIO27', nodePin: 'G43', shared: false },
        { signal: 'GND', hubPin: 'GND', nodePin: 'GND', shared: true },
      ],
    },
  ],
  order: ['seg', 'watch', 'hub'],
};

/** dagon-host target key of a node, as the mock and discovery name it. */
export function targetKey(t: Target): string {
  return t.kind === 'i2c' ? `i2c:${t.addr}` : t.kind;
}
