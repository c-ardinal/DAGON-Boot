//! Argument rules of the hub commands, kept identical to the simulated hub
//! (`dagon-sim`) so tools tested against it behave the same on the board.

use dagon_proto::payload::{link, I2cRaw};
use dagon_proto::{frame_len, max_payload, Status};

/// RELAY targets the hub accepts: I2C0 with a 7-bit address and a frame
/// within the I2C limit, or the PIO UART (`addr = 0xFF`) within the UART
/// limit. Anything else answers E_ARG.
pub fn relay_fits(l: u8, addr: u8, inner_len: usize) -> bool {
    match (l, addr) {
        (link::I2C0, a) => a < 0x80 && inner_len <= frame_len(max_payload::I2C),
        (link::PIO_UART, link::UART_ADDR) => inner_len <= frame_len(max_payload::UART),
        _ => false,
    }
}

/// Longest I2C_RAW read that fits the USB response.
pub const RAW_READ_MAX: usize = max_payload::USB - 1;

/// I2C_RAW: parse errors as parsed, E_STATE unless BOOT0 is high, E_ARG for
/// a read longer than [`RAW_READ_MAX`].
pub fn i2c_raw(p: &[u8], boot0_high: bool) -> Result<I2cRaw<'_>, Status> {
    let raw = I2cRaw::parse(p)?;
    if !boot0_high {
        return Err(Status::State);
    }
    if raw.read_len as usize > RAW_READ_MAX {
        return Err(Status::Arg);
    }
    Ok(raw)
}
