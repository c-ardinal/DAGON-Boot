//! The ESP32-S3 ROM loader procedure (`dagon_romlink::esp::flash`) against
//! the loader model, over a byte stream.

use dagon_port::{BusError, StreamPort};
use dagon_romlink::esp::{self, cmd, EspError, EspLink, Plan};
use dagon_romlink::{HostClock, Region};
use dagon_sim::esp_rom::{self, SimEspRom};
use dagon_sim::{block_on, SimFlash};

struct Wire {
    rom: Option<SimEspRom>,
    flash: SimFlash,
    now_ms: u64,
    progress: Vec<u8>,
    /// Flips one bit of the data of the FLASH_DATA with this index.
    corrupt: Option<u32>,
    data_sent: u32,
}

impl Wire {
    fn new(rom: Option<SimEspRom>) -> Self {
        Wire {
            rom,
            flash: SimFlash::esp32s3_4m(1),
            now_ms: 0,
            progress: Vec::new(),
            corrupt: None,
            data_sent: 0,
        }
    }
}

impl StreamPort for Wire {
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError> {
        match self.rom.as_mut().map(|r| r.read(buf)) {
            Some(n) if n > 0 => Ok(n),
            _ => {
                self.now_ms += u64::from(timeout_ms);
                Err(BusError::Timeout)
            }
        }
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
        let mut data = data.to_vec();
        // Byte 1 of a frame is the direction, byte 2 the command.
        if data.get(2) == Some(&cmd::FLASH_DATA) {
            if self.corrupt == Some(self.data_sent) {
                let at = data.len() - 2;
                data[at] ^= 1;
            }
            self.data_sent += 1;
        }
        if let Some(rom) = &mut self.rom {
            rom.write(&mut self.flash, &data, &mut self.now_ms);
        }
        Ok(())
    }
}

impl HostClock for Wire {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.now_ms += u64::from(ms);
    }
}

impl EspLink for Wire {
    async fn progress(&mut self, percent: u8) {
        self.progress.push(percent);
    }
}

fn image(len: usize, salt: u8) -> Vec<u8> {
    // Every byte value, 0xC0 and 0xDB included.
    (0..len).map(|i| (i as u8).wrapping_mul(7) ^ salt).collect()
}

fn plan<'a>(regions: &'a [Region<'a>]) -> Plan<'a> {
    Plan {
        flash_bytes: 0x40_0000,
        regions,
        reboot: true,
    }
}

#[test]
fn regions_are_written_in_order_and_the_loader_reboots() {
    let (body, header) = (image(5000, 0), image(0x1000, 0x5A));
    let regions = [
        Region {
            addr: 0x1_0000,
            data: &body,
        },
        Region {
            addr: 0x2000,
            data: &header,
        },
    ];
    let mut w = Wire::new(Some(SimEspRom::started(b"")));
    // What the erase must remove, and what it must leave alone.
    dagon_port::Flash::write(&mut w.flash, 0x1_0000, &[0u8; 8]).unwrap();
    dagon_port::Flash::write(&mut w.flash, 0x1_2000, &[0u8; 8]).unwrap();

    let stats = block_on(esp::flash(&mut w, &plan(&regions))).unwrap();

    assert_eq!(w.flash.peek(0x1_0000, 5000).unwrap(), &body[..]);
    assert_eq!(w.flash.peek(0x1_0000 + 5000, 4).unwrap(), &[0xFF; 4]);
    assert_eq!(w.flash.peek(0x1_2000, 8).unwrap(), &[0u8; 8]);
    assert_eq!(w.flash.peek(0x2000, 0x1000).unwrap(), &header[..]);
    assert_eq!((stats.syncs, stats.blocks), (1, 5 + 4));
    assert_eq!(w.progress.last(), Some(&100));
    assert!(w.progress.windows(2).all(|p| p[0] <= p[1]));
    let rom = w.rom.unwrap();
    assert!(rom.rebooted());
    // The header's region begins only after the body's MD5 was asked.
    let seen = rom.seen();
    let md5 = seen.iter().position(|&c| c == cmd::SPI_FLASH_MD5).unwrap();
    let second = seen.iter().rposition(|&c| c == cmd::FLASH_BEGIN).unwrap();
    assert!(md5 < second);
    assert_eq!(seen.last(), Some(&cmd::FLASH_END));
}

#[test]
fn the_loader_stays_when_no_reboot_is_asked() {
    let data = image(64, 1);
    let regions = [Region {
        addr: 0,
        data: &data,
    }];
    let mut w = Wire::new(Some(SimEspRom::started(b"")));
    let plan = Plan {
        reboot: false,
        ..plan(&regions)
    };
    block_on(esp::flash(&mut w, &plan)).unwrap();
    assert!(!w.rom.unwrap().rebooted());
}

#[test]
fn the_boot_log_before_the_first_answer_is_read_past() {
    let data = image(2048, 2);
    let regions = [Region {
        addr: 0x8000,
        data: &data,
    }];
    let log = b"ESP-ROM:esp32s3-20210327\r\n\xC0\x01boot:0x0 (DOWNLOAD(USB/UART0))\r\nwaiting for download\r\n";
    let mut w = Wire::new(Some(SimEspRom::started(log)));
    block_on(esp::flash(&mut w, &plan(&regions))).unwrap();
    assert_eq!(w.flash.peek(0x8000, 2048).unwrap(), &data[..]);
}

#[test]
fn a_stream_with_no_loader_ends_as_no_sync() {
    let regions = [Region {
        addr: 0,
        data: &[1, 2, 3, 4],
    }];
    let mut w = Wire::new(None);
    assert_eq!(
        block_on(esp::flash(&mut w, &plan(&regions))),
        Err(EspError::NoSync)
    );
    assert_eq!(
        w.now_ms,
        u64::from(esp::SYNC_ATTEMPTS) * u64::from(esp::SYNC_WAIT_MS)
    );
}

#[test]
fn a_block_damaged_on_the_wire_is_refused_by_its_checksum() {
    let data = image(3000, 3);
    let regions = [Region {
        addr: 0x4000,
        data: &data,
    }];
    let mut w = Wire::new(Some(SimEspRom::started(b"")));
    w.corrupt = Some(1);
    assert_eq!(
        block_on(esp::flash(&mut w, &plan(&regions))),
        Err(EspError::Failed {
            cmd: cmd::FLASH_DATA,
            code: esp_rom::BAD_CHECKSUM
        })
    );
    // The damaged block was not programmed.
    assert_eq!(w.flash.peek(0x4000 + 1024, 4).unwrap(), &[0xFF; 4]);
}

#[test]
fn a_region_past_the_flash_size_is_refused() {
    let data = image(16, 4);
    let regions = [Region {
        addr: 0x3F_FFF8,
        data: &data,
    }];
    let mut w = Wire::new(Some(SimEspRom::started(b"")));
    assert_eq!(
        block_on(esp::flash(&mut w, &plan(&regions))),
        Err(EspError::Failed {
            cmd: cmd::FLASH_BEGIN,
            code: esp_rom::FAILED
        })
    );
}

/// A flash that keeps other content than was sent: the MD5 differs.
#[test]
fn content_that_differs_after_the_write_is_a_mismatch() {
    let data = image(1024, 5);
    let regions = [Region {
        addr: 0x6000,
        data: &data,
    }];
    struct Rot(Wire);
    impl StreamPort for Rot {
        async fn read(&mut self, buf: &mut [u8], t: u32) -> Result<usize, BusError> {
            self.0.read(buf, t).await
        }
        async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
            if data.get(2) == Some(&cmd::SPI_FLASH_MD5) {
                // The sector loses its content after the write.
                dagon_port::Flash::erase(&mut self.0.flash, 0x6000, 0x1000).unwrap();
            }
            self.0.write(data).await
        }
    }
    impl HostClock for Rot {
        fn now_ms(&self) -> u64 {
            self.0.now_ms
        }
        async fn sleep_ms(&mut self, ms: u32) {
            self.0.sleep_ms(ms).await
        }
    }
    impl EspLink for Rot {}
    let mut w = Rot(Wire::new(Some(SimEspRom::started(b""))));
    let got = block_on(esp::flash(&mut w, &plan(&regions)));
    let changed = w.0.flash.peek(0x6000, 1024).unwrap() != &data[..];
    assert!(changed, "the test's own damage took no effect");
    assert_eq!(got, Err(EspError::Mismatch(0x6000)));
}

#[test]
fn md5_matches_the_rfc_1321_test_suite() {
    let hex = |d: &[u8]| -> String { esp::md5(d).iter().map(|b| format!("{b:02x}")).collect() };
    assert_eq!(hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        hex(b"abcdefghijklmnopqrstuvwxyz"),
        "c3fcd3d76192e4007dfb496cca67e13b"
    );
    assert_eq!(
        hex(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"),
        "57edf4a22be3c955ac49da2e2107b67a"
    );
    // 56 bytes: the length no longer fits the first padded block.
    assert_eq!(
        hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "8215ef0796a20bcaaae116d3876c664a"
    );
}

#[test]
fn a_request_frame_escapes_both_special_bytes() {
    let mut out = [0u8; 64];
    let n = esp::request(&mut out, 0x03, 0xC0, &[0xDB], &[0x01, 0xC0]);
    assert_eq!(
        &out[..n],
        &[
            0xC0, 0x00, 0x03, 0x03, 0x00, 0xDB, 0xDC, 0x00, 0x00, 0x00, 0xDB, 0xDD, 0x01, 0xDB,
            0xDC, 0xC0
        ]
    );
    assert_eq!(esp::checksum(&[]), 0xEF);
    assert_eq!(esp::checksum(&[0xEF, 0x01]), 0x01);
}
