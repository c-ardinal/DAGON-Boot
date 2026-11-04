//! DMA request mapping: which stream and request channel of which DMA
//! controller serve each request of a register block, as the MCU's
//! reference manual tables them (STM32F401: RM0368, DMA1 and DMA2 request
//! mapping). A hardware fact of the MCU, so it is MCU definition data;
//! board packages and MCU templates take their streams from it
//! ([`assign`]) instead of naming them in code.
//!
//! ```toml
//! [[peripheral]]
//! id = "dma1"
//! dma = { streams = 8, channels = 8 }
//!
//! [[peripheral]]
//! id = "i2c3"
//! dma_requests = [{ signal = "rx", controller = "dma1", stream = 1, channel = 1 }]
//! ```

use serde::Deserialize;

use super::board_boot::BusKind;
use super::{BoardBus, BoardDef, McuDef};

/// A DMA controller: `streams` streams, each selecting one of `channels`
/// request channels at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub streams: u8,
    pub channels: u8,
}

/// One request of a block: its `signal` (`tx`, `rx`, a timer's `ch1`,
/// `up`, …) on `stream` of `controller`, selected as request `channel`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub signal: String,
    pub controller: String,
    pub stream: u8,
    pub channel: u8,
}

/// The stream a use of a request got from [`assign`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    pub controller: String,
    pub stream: u8,
    pub channel: u8,
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

/// `dma` and `dma_requests` of every block: controllers are bus masters
/// with at least one stream and channel; each request names a controller,
/// a stream and a channel it has; one cell (controller, stream, channel)
/// serves one block only, and no request is listed twice.
pub(super) fn check(m: &McuDef) -> Result<(), String> {
    for p in &m.peripherals {
        if let Some(c) = p.dma {
            fail(!p.bus_master, || {
                format!("peripheral {}: a DMA controller is a bus_master", p.id)
            })?;
            fail(c.streams == 0 || c.channels == 0, || {
                format!(
                    "peripheral {}: dma streams and channels must be above 0",
                    p.id
                )
            })?;
        }
    }
    let mut cells: Vec<(&str, &Request)> = Vec::new();
    for p in &m.peripherals {
        for r in &p.dma_requests {
            let at = format!("peripheral {} dma_requests {}", p.id, r.signal);
            fail(
                r.signal.is_empty()
                    || !r
                        .signal
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
                || format!("{at}: a signal is a-z 0-9"),
            )?;
            let c = m
                .peripheral(&r.controller)
                .and_then(|c| c.dma)
                .ok_or_else(|| format!("{at}: {} is no DMA controller", r.controller))?;
            fail(r.stream >= c.streams || r.channel >= c.channels, || {
                format!(
                    "{at}: {} has streams 0..{} and channels 0..{}",
                    r.controller, c.streams, c.channels
                )
            })?;
            let same_cell = |o: &&(&str, &Request)| {
                o.1.controller == r.controller && o.1.stream == r.stream && o.1.channel == r.channel
            };
            if let Some((other, o)) = cells.iter().find(same_cell) {
                fail(*other != p.id, || {
                    format!(
                        "{at}: {} stream {} channel {} also serves {other}",
                        r.controller, r.stream, r.channel
                    )
                })?;
                fail(o.signal == r.signal, || format!("{at}: listed twice"))?;
            }
            cells.push((&p.id, r));
        }
    }
    Ok(())
}

/// Streams for `uses` (block id, signal), in order: each use gets the
/// first request of its block's table (the definition's order) on a stream
/// no earlier use took, since a stream serves one request at a time. Fails
/// naming the use when its block lists no such request or every stream
/// that serves it is taken.
pub fn assign(m: &McuDef, uses: &[(&str, &str)]) -> Result<Vec<Stream>, String> {
    let mut out: Vec<Stream> = Vec::new();
    for (block, signal) in uses {
        let p = m
            .peripheral(block)
            .ok_or_else(|| format!("{}: no peripheral {block}", m.id))?;
        let mut listed = p.dma_requests.iter().filter(|r| r.signal == *signal);
        let first = listed.clone().next();
        let r = listed
            .find(|r| {
                !out.iter()
                    .any(|s| s.controller == r.controller && s.stream == r.stream)
            })
            .ok_or_else(|| match first {
                None => format!("{}: {block} lists no DMA request {signal}", m.id),
                Some(_) => format!(
                    "{}: every DMA stream serving {block} {signal} is taken",
                    m.id
                ),
            })?;
        out.push(Stream {
            controller: r.controller.clone(),
            stream: r.stream,
            channel: r.channel,
        });
    }
    Ok(out)
}

/// A bus the board package opens with DMA, and its streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusStreams<'a> {
    pub bus: &'a BoardBus,
    pub tx: Stream,
    pub rx: Stream,
}

/// The streams of every `dma = true` bus of board `b`: buses in the
/// definition's order, tx then rx each ([`assign`]).
pub fn board_streams<'a>(b: &'a BoardDef, m: &McuDef) -> Result<Vec<BusStreams<'a>>, String> {
    let buses: Vec<&BoardBus> = b.buses.iter().filter(|x| x.dma).collect();
    if let Some(x) = buses.iter().find(|x| x.kind == BusKind::Can) {
        return Err(format!("bus {}: dma is for i2c, spi and uart buses", x.id));
    }
    let uses: Vec<(&str, &str)> = buses
        .iter()
        .flat_map(|x| [(x.controller.as_str(), "tx"), (x.controller.as_str(), "rx")])
        .collect();
    let mut got = assign(m, &uses)?.into_iter();
    Ok(buses
        .into_iter()
        .map(|bus| BusStreams {
            bus,
            tx: got.next().expect("one per use"),
            rx: got.next().expect("one per use"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::Registry;

    fn f401() -> McuDef {
        Registry::builtin().mcu("stm32f401re").unwrap().clone()
    }

    fn at(s: &Stream) -> (&str, u8, u8) {
        (s.controller.as_str(), s.stream, s.channel)
    }

    #[test]
    fn streams_come_from_the_table_first_free_first() {
        let m = f401();
        check(&m).unwrap();
        let got = assign(
            &m,
            &[
                ("i2c3", "tx"),
                ("i2c3", "rx"),
                ("i2c1", "tx"),
                ("i2c1", "rx"),
            ],
        )
        .unwrap();
        let got: Vec<_> = got.iter().map(at).collect();
        assert_eq!(
            got,
            [
                ("dma1", 4, 3),
                ("dma1", 1, 1),
                ("dma1", 6, 1),
                ("dma1", 0, 1)
            ]
        );
        // A taken stream moves the next use to the request's other one.
        let got = assign(&m, &[("i2c1", "rx"), ("i2c3", "rx"), ("i2c3", "rx")]).unwrap();
        let got: Vec<_> = got.iter().map(at).collect();
        assert_eq!(got, [("dma1", 0, 1), ("dma1", 1, 1), ("dma1", 2, 3)]);
        // USART2 TX has stream 6 only.
        let e = assign(&m, &[("i2c1", "tx"), ("usart2", "tx")]).unwrap_err();
        assert!(
            e.contains("every DMA stream serving usart2 tx is taken"),
            "{e}"
        );
        let e = assign(&m, &[("spi4", "tx")]).unwrap_err();
        assert!(e.contains("lists no DMA request tx"), "{e}");
        assert!(assign(&m, &[("nope", "tx")]).is_err());
    }

    #[test]
    fn a_board_gets_streams_for_its_dma_buses_only() {
        let reg = Registry::builtin();
        let mut b = reg.board("nucleo-f401re").unwrap().clone();
        let m = f401();
        let got = board_streams(&b, &m).unwrap();
        let ids: Vec<&str> = got.iter().map(|x| x.bus.id.as_str()).collect();
        assert_eq!(ids, ["link", "arduino-i2c"]);
        assert_eq!(at(&got[0].rx), ("dma1", 1, 1));
        // The VCP's TX (USART2, stream 6 only) meets the Arduino I2C's.
        let vcp = b.buses.iter().position(|x| x.id == "vcp").unwrap();
        b.buses[vcp].dma = true;
        let e = board_streams(&b, &m).unwrap_err();
        assert!(e.contains("usart2 tx is taken"), "{e}");
        let e = crate::defs::requirements::check_board(&b, &m).unwrap_err();
        assert!(e.contains("DMA stream assignment"), "{e}");
        b.buses[vcp].kind = BusKind::Can;
        assert!(board_streams(&b, &m)
            .unwrap_err()
            .contains("i2c, spi and uart"));
    }

    #[test]
    fn requests_are_checked_against_their_controller() {
        let broken = |f: &dyn Fn(&mut McuDef)| {
            let mut m = f401();
            f(&mut m);
            check(&m).unwrap_err()
        };
        let i2c3 = |m: &mut McuDef| m.peripherals.iter().position(|p| p.id == "i2c3").unwrap();
        let e = broken(&|m| {
            let i = i2c3(m);
            m.peripherals[i].dma_requests[0].stream = 8;
        });
        assert!(e.contains("streams 0..8"), "{e}");
        let e = broken(&|m| {
            let i = i2c3(m);
            m.peripherals[i].dma_requests[0].controller = "i2c1".into();
        });
        assert!(e.contains("no DMA controller"), "{e}");
        let e = broken(&|m| {
            let i = i2c3(m);
            m.peripherals[i].dma_requests[0].signal = "RX".into();
        });
        assert!(e.contains("a-z 0-9"), "{e}");
        let e = broken(&|m| {
            let i = i2c3(m);
            let dup = m.peripherals[i].dma_requests[0].clone();
            m.peripherals[i].dma_requests.push(dup);
        });
        assert!(e.contains("listed twice"), "{e}");
        // I2C1 RX's cell (stream 0, channel 1) claimed by I2C3 too.
        let e = broken(&|m| {
            let i = i2c3(m);
            m.peripherals[i].dma_requests[0].stream = 0;
        });
        assert!(e.contains("also serves"), "{e}");
        let e = broken(&|m| {
            let d = m.peripherals.iter().position(|p| p.id == "dma1").unwrap();
            m.peripherals[d].bus_master = false;
        });
        assert!(e.contains("bus_master"), "{e}");
    }
}
