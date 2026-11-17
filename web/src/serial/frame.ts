// DAGON link frame: SOF | ver | flags | seq | cmd | len u16 LE |
// payload | crc32 LE. Encoding, CRC and the stream cutter come from
// dagon-wasm (crates/dagon-proto); this file only names fields and splits a
// frame that has already been checked.
import { encodeFrameBytes, frameStream, wireNumbers, type FrameStream } from '../wasm/dagon.ts';

const n = () => wireNumbers();

/** Frame sizes, payload limits and flags, as dagon-proto has them. */
export const WIRE = {
  get HEADER_LEN() {
    return n().headerLen;
  },
  get CRC_LEN() {
    return n().crcLen;
  },
  get MAX_PAYLOAD_USB() {
    return n().maxPayloadUsb;
  },
  get MAX_PAYLOAD_UART() {
    return n().maxPayloadUart;
  },
  get MAX_PAYLOAD_I2C() {
    return n().maxPayloadI2c;
  },
  get FLAG_RESP() {
    return n().flagResp;
  },
};

/** Command numbers, as dagon-proto's command table has them. */
export const CMD = {
  get HELLO() {
    return n().cmdHello;
  },
  get STATUS() {
    return n().cmdStatus;
  },
  get XFER_BEGIN() {
    return n().cmdXferBegin;
  },
  get XFER_DATA() {
    return n().cmdXferData;
  },
  get XFER_END() {
    return n().cmdXferEnd;
  },
  get XFER_HDR() {
    return n().cmdXferHdr;
  },
  get SET_PENDING() {
    return n().cmdSetPending;
  },
  get REBOOT() {
    return n().cmdReboot;
  },
  get CONFIRM() {
    return n().cmdConfirm;
  },
  get SHOW() {
    return n().cmdShow;
  },
  get RELAY() {
    return n().cmdRelay;
  },
};

/** Status numbers, as dagon-proto has them. */
export const STATUS = {
  get OK() {
    return n().statusOk;
  },
  get UNSUPPORTED() {
    return n().statusUnsupported;
  },
  get ARG() {
    return n().statusArg;
  },
  get FIELD() {
    return n().statusField;
  },
  get STATE() {
    return n().statusState;
  },
  get TIMEOUT() {
    return n().statusTimeout;
  },
};

export interface Frame {
  flags: number;
  seq: number;
  cmd: number;
  payload: Uint8Array;
}

export function encodeFrame(f: Frame): Uint8Array {
  return encodeFrameBytes(f.flags, f.seq, f.cmd, f.payload);
}

/** Splits a frame that came from {@link newFrameStream} or {@link encodeFrame}. */
export function parseFrame(raw: Uint8Array): Frame {
  return { flags: raw[3], seq: raw[4], cmd: raw[5], payload: raw.slice(WIRE.HEADER_LEN, raw.length - WIRE.CRC_LEN) };
}

/** Cuts complete, CRC-checked frames out of a byte stream (dagon-wasm). */
export function newFrameStream(maxPayload = WIRE.MAX_PAYLOAD_USB): FrameStream {
  return frameStream(maxPayload);
}
