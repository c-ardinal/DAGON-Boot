// DAGON link v1 frame: SOF DA 6F | ver 01 | flags | seq | cmd | len u16 LE |
// payload | crc32 LE. Encoding, CRC and the stream cutter come from
// dagon-wasm (crates/dagon-proto); this file only names fields and splits a
// frame that has already been checked.
import { encodeFrameBytes, frameStream, type FrameStream } from '../wasm/dagon.ts';

export const HEADER_LEN = 8;
export const CRC_LEN = 4;
export const MAX_PAYLOAD_USB = 1088;

export const FLAG_RESP = 1 << 0;

export const CMD = {
  HELLO: 0x01,
  STATUS: 0x02,
} as const;

export const STATUS_OK = 0x00;

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
  return { flags: raw[3], seq: raw[4], cmd: raw[5], payload: raw.slice(HEADER_LEN, raw.length - CRC_LEN) };
}

/** Cuts complete, CRC-checked frames out of a byte stream (dagon-wasm). */
export function newFrameStream(maxPayload = MAX_PAYLOAD_USB): FrameStream {
  return frameStream(maxPayload);
}
