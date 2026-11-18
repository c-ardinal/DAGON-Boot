// The sample 構成 (topology/sample.yaml): coordinator, 7-segment node on I2C and watch on
// UART, with the names the page uses for them and the physical wiring of each
// link. The YAML is the repository's sample topology; it is what "send to the
// coordinator" compiles and sends.
import sampleYaml from '../../../topology/sample.yaml?raw';
import { LinkKind } from '../wasm/pkg/dagon_wasm.js';
import type { Kind } from './define/model.ts';

export const SAMPLE_YAML: string = sampleYaml;

export interface Node {
  id: string;
  /** Number shown as "[2]". */
  no: number;
  /** Name on screen. */
  name: string;
  role: 'hub' | 'node';
  product: string;
  mcu: string;
  /** The MCU's id in the definitions (its slots come from there); absent: every STATUS entry is shown. */
  mcuId?: string;
  /** Link value shown on the node: "I2C 0x42", "UART 115200"; empty for the coordinator. */
  via: string;
  /** Coordinator pins driving reset and boot; null when the node switches itself. */
  reset: string | null;
  boot: string | null;
  /** Row on the graph: 0 coordinator, 1 node, 2 and on deeper. Absent: by role. */
  depth?: number;
  /** A coordinator's uplink as the 構成 writes it; absent when none is written (USB to the PC) or the coordinator is linked from another node. */
  uplink?: { kind: Kind; host: UplinkHost };
  /** Why updating skips this node (V-23); absent when updating reaches it. */
  skip?: Skip;
}

/** What drives a coordinator through its uplink. */
export type UplinkHost = 'pc-cli' | 'browser' | 'embedded';

/** dagon_topology::Skip codes: why updating does not reach a node. */
export type Skip = 'second-hub' | 'deep' | 'link-kind' | 'unreachable' | 'cut-invalid';

export interface Wire {
  signal: string;
  hubPin: string;
  nodePin: string;
  shared: boolean;
}

export interface Link {
  from: string;
  to: string;
  kind: Kind;
  /** Address value for the spoken name ("0x42"), empty for UART. */
  addr: string;
  wires: Wire[];
}

export interface Config {
  name: string;
  yaml: string;
  nodes: Node[];
  links: Link[];
  /** Node ids, the other nodes first and the coordinator last. */
  order: string[];
  /**
   * Ids of the nodes updating reaches, in the compiled topology's order
   * (dagon_topology::cut_with through dagon-wasm): dagon-host's node index
   * is the place here. Every other node carries `skip`.
   */
  reach: string[];
}

const NRST_HEADER = 'NRST（ヘッダの位置: 未設定）';
const BOOT0_HEADER = 'BOOT0（ヘッダの位置: 未設定）';

export const SAMPLE_CONFIG: Config = {
  name: 'sample',
  yaml: SAMPLE_YAML,
  nodes: [
    { id: 'hub', no: 1, name: 'メイン', role: 'hub', product: 'Waveshare RP2040-Touch-LCD-1.28', mcu: 'RP2040', mcuId: 'rp2040', via: '', reset: null, boot: null },
    { id: 'seg', no: 2, name: '7セグ', role: 'node', product: 'NUCLEO-F401RE', mcu: 'STM32F401RE', mcuId: 'stm32f401re', via: 'I2C 0x42', reset: 'GPIO18', boot: 'GPIO28' },
    { id: 'watch', no: 3, name: 'ウォッチ', role: 'node', product: 'M5StopWatch', mcu: 'ESP32-S3', mcuId: 'esp32s3', via: 'UART 115200', reset: null, boot: null },
  ],
  links: [
    {
      from: 'hub',
      to: 'seg',
      kind: LinkKind.I2c,
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
      kind: LinkKind.Uart,
      addr: '',
      wires: [
        { signal: 'TX → RX', hubPin: 'GPIO26', nodePin: 'G44', shared: false },
        { signal: 'RX ← TX', hubPin: 'GPIO27', nodePin: 'G43', shared: false },
        { signal: 'GND', hubPin: 'GND', nodePin: 'GND', shared: true },
      ],
    },
  ],
  order: ['seg', 'watch', 'hub'],
  reach: ['hub', 'seg', 'watch'],
};
