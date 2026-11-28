//! The HAL side of the board: peripheral split, interrupts, the relay
//! start and the link and display bus set-up.

use dagon_mcu_rp2040::links::{I2cLink, Kind, PioStream, RpI2c};
pub use dagon_mcu_rp2040::node::DagonParts;
use dagon_mcu_rp2040::sub::SubLines;
use dagon_mcu_rp2040::EraseRegion;
pub use dagon_mcu_rp2040::{BoardCore, BoardDef};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::interrupt::typelevel::{Binding, DMA_IRQ_0, I2C0_IRQ, PIO0_IRQ_0};
use embassy_rp::peripherals::*;
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::spi::{self, Async, Spi};
use embassy_rp::{bind_interrupts, dma, Peri, Peripherals};

bind_interrupts!(pub struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

use crate::def::pins::{
    Backlight, ChildBoot0, ChildI2c, ChildI2cScl, ChildI2cSda, ChildNrst, ChildUart, ChildUartRx,
    ChildUartTx, Lcd, LcdCs, LcdDc, LcdMosi, LcdRst, LcdSck, TouchRst,
};
use crate::{BOARD_ID, DIAG_LED, FLASH_SIZE, I2C_HZ, LCD_SPI_HZ, UART_BAUD};

pub struct Board;

/// Moves DAGON's peripherals (flash, watchdog, USB) out of
/// `embassy_rp::Peripherals`; the rest stays the app's.
#[macro_export]
macro_rules! dagon_pins {
    ($p:ident) => {
        $crate::DagonParts {
            flash: $p.FLASH,
            watchdog: $p.WATCHDOG,
            usb: $p.USB,
        }
    };
}

/// Starts DAGON's relay on this board: up over USB, or over the PIO UART
/// on a node reached over UART; down over I2C0, the PIO UART and the
/// STM32 control lines (PC and link tasks on DAGON's high-priority
/// executor, the feeder on `spawner`), with the touch controller held out
/// of reset. Declare the guard with `dagon_mcu_rp2040::relay::levels()`;
/// the app's tasks take the guard tasks from
/// `dagon_mcu_rp2040::relay::TASKS` on. Returns what DAGON does not use,
/// the display's pins among it, for the app's own tasks, and the node's
/// events (what an app draws a display from). The board has no bring-up,
/// so no `app.bring_up` is emitted.
#[cfg(feature = "relay")]
pub fn start<const N: usize>(
    spawner: embassy_executor::Spawner,
    dagon: DagonParts,
    io: Io,
    guard: &'static dagon_mcu_rp2040::watchdog::Guard<N>,
    setup: dagon_mcu_rp2040::relay::Setup,
    uplink: Kind,
    downlinks: &[Kind],
) -> (Spare, dagon_mcu_rp2040::node::AppEvents) {
    use dagon_mcu_rp2040::relay::{UartUp, Uplink};
    assert!(
        downlinks
            .iter()
            .all(|k| matches!(k, Kind::I2c | Kind::Uart)),
        "downlinks: only i2c and uart on this board"
    );
    let wants = |k: Kind| downlinks.contains(&k);
    // The uplink (`require_uplink` checks the kind at compile time): the
    // USB CDC device, built by the relay, or on a node reached over UART
    // the PIO UART of bus `child-uart`, where its boot core receives too.
    // That bus is then no downlink.
    let mut sub_uart = Some(io.sub_uart);
    let up = match uplink {
        Kind::UsbCdc => Uplink::Usb,
        Kind::Uart => {
            assert!(
                !wants(Kind::Uart),
                "uart: the uplink takes bus child-uart, so no uart downlink on this board"
            );
            let bus = sub_uart.take().expect("the bus is taken once");
            Uplink::uart(UartUp::pio0(downlink_uart(bus)))
        }
        _ => panic!("uplink: usb-cdc or uart on this board"),
    };
    // The STM32 recovery lines belong to the I2C downlink (ROM path): the
    // node on bus `child-i2c`.
    let lines = wants(Kind::I2c)
        .then(|| dagon_mcu_rp2040::relay::RecoveryLines::on_i2c0(io.sub_ctl.lines()));
    let links = dagon_mcu_rp2040::relay::Links {
        up,
        lines: [lines, None],
        uart: sub_uart.filter(|_| wants(Kind::Uart)).map(downlink_uart),
        i2c: wants(Kind::I2c).then(|| downlink_i2c(io.sub_i2c)),
        ..dagon_mcu_rp2040::relay::Links::none()
    };
    // The touch controller stays out of reset for the life of the app.
    core::mem::forget(hold_touch(io.touch_rst));
    let events = dagon_mcu_rp2040::relay::start::<Board, N>(spawner, dagon, links, guard, setup);
    (io.spare, events)
}

/// Down `i2c` on this board: bus `child-i2c` at [`I2C_HZ`] with the
/// DAGON mailbox.
pub fn downlink_i2c(p: SubI2c) -> I2cLink<RpI2c<'static, I2C0>> {
    p.link(Irqs)
}

/// Down `uart` on this board: the PIO UART of bus `child-uart` at [`UART_BAUD`]
/// (the relay wraps it in `links::Uart` with its clock).
pub fn downlink_uart(p: SubUart) -> PioStream<'static, PIO0, 0, 1> {
    p.stream(Irqs)
}

impl BoardCore for Board {
    const BOARD_ID: &'static str = BOARD_ID;
    const FLASH_SIZE: usize = FLASH_SIZE;
}

impl BoardDef for Board {
    const ERASE_REGIONS: &'static [EraseRegion] = &EraseRegion::table(crate::def::ERASE_REGIONS);
    const DIAG_LED: Option<u8> = Some(DIAG_LED);
    const DIAG_LED_ACTIVE_HIGH: bool = crate::DIAG_LED_ACTIVE_HIGH;
    type UserIo = Io;

    fn split(p: Peripherals) -> (DagonParts, Io) {
        let dagon = DagonParts {
            flash: p.FLASH,
            watchdog: p.WATCHDOG,
            usb: p.USB,
        };
        let display = Display {
            spi: take!(p.lcd),
            clk: take!(p.lcd_sck),
            mosi: take!(p.lcd_mosi),
            dma: p.DMA_CH0,
            cs: take!(p.lcd_cs),
            dc: take!(p.lcd_dc),
            rst: take!(p.lcd_rst),
            backlight: take!(p.backlight),
        };
        let io = Io {
            touch_rst: take!(p.touch_rst),
            sub_i2c: SubI2c {
                bus: take!(p.child_i2c),
                sda: take!(p.child_i2c_sda),
                scl: take!(p.child_i2c_scl),
            },
            sub_uart: SubUart {
                pio: take!(p.child_uart),
                tx: take!(p.child_uart_tx),
                rx: take!(p.child_uart_rx),
            },
            sub_ctl: SubCtl {
                nrst: take!(p.child_nrst),
                boot0: take!(p.child_boot0),
            },
            spare: take_spare_gpios!(
                p,
                Spare {
                    i2c1: p.I2C1,
                    spi0: p.SPI0,
                    uart0: p.UART0,
                    uart1: p.UART1,
                    pio1: p.PIO1,
                    adc: p.ADC,
                    dma_ch1: p.DMA_CH1,
                    dma_ch2: p.DMA_CH2,
                    dma_ch3: p.DMA_CH3,
                    dma_ch4: p.DMA_CH4,
                    dma_ch5: p.DMA_CH5,
                    dma_ch6: p.DMA_CH6,
                    dma_ch7: p.DMA_CH7,
                    dma_ch8: p.DMA_CH8,
                    dma_ch9: p.DMA_CH9,
                    dma_ch10: p.DMA_CH10,
                    dma_ch11: p.DMA_CH11,
                    pwm_slice0: p.PWM_SLICE0,
                    pwm_slice1: p.PWM_SLICE1,
                    pwm_slice2: p.PWM_SLICE2,
                    pwm_slice3: p.PWM_SLICE3,
                    pwm_slice4: p.PWM_SLICE4,
                    pwm_slice5: p.PWM_SLICE5,
                    pwm_slice6: p.PWM_SLICE6,
                    pwm_slice7: p.PWM_SLICE7,
                    core1: p.CORE1,
                    display,
                }
            ),
        };
        (dagon, io)
    }
}

/// What the app gets on this board.
pub struct Io {
    /// Touch controller reset; see [`hold_touch`].
    pub touch_rst: Peri<'static, TouchRst>,
    pub sub_i2c: SubI2c,
    pub sub_uart: SubUart,
    pub sub_ctl: SubCtl,
    /// Everything DAGON does not use; [`start`] hands it back to the app.
    pub spare: Spare,
}

/// Declares [`Spare`] with a field for each GPIO the board's definition
/// names for nothing (`spare_gpios!`), then the blocks below.
macro_rules! spare {
    ($($pin:ident $ty:ident,)*) => {
        #[doc = "The pins and peripherals DAGON's relay does not use, the display's"]
        #[doc = "among them; [`start`] returns them for the app. The pins are each"]
        #[doc = "GPIO the board's definition (with a project's overrides) names for"]
        #[doc = "nothing, so a pin moved to a free one leaves this list and the pin it"]
        #[doc = "left joins it. Some of them are wired on the board itself:"]
        #[doc = ""]
        #[doc = "| Pin | On the board |"]
        #[doc = "|---|---|"]
        #[doc = "| `pin_6`, `pin_7` | I2C1 SDA, SCL of the touch controller (CST816S) and the IMU (QMI8658) |"]
        #[doc = "| `pin_21` | touch controller interrupt |"]
        #[doc = "| `pin_23`, `pin_24` | IMU interrupts INT1, INT2 |"]
        #[doc = "| `pin_12` | LCD MISO (SPI1 RX; [`Display::pins`] sets the bus up to write only) |"]
        #[doc = "| `pin_29` | battery voltage (ADC3, through a divider) |"]
        #[doc = ""]
        #[doc = "The others (`pin_0`..`pin_5`, `pin_14`, `pin_15`, `pin_19`, `pin_20`"]
        #[doc = "on the built-in definition) are free for the app; check the board's"]
        #[doc = "pinout for which reach a header."]
        pub struct Spare {
            $(pub $pin: Peri<'static, $ty>,)*
            pub i2c1: Peri<'static, I2C1>,
            pub spi0: Peri<'static, SPI0>,
            pub uart0: Peri<'static, UART0>,
            pub uart1: Peri<'static, UART1>,
            pub pio1: Peri<'static, PIO1>,
            pub adc: Peri<'static, ADC>,
            pub dma_ch1: Peri<'static, DMA_CH1>,
            pub dma_ch2: Peri<'static, DMA_CH2>,
            pub dma_ch3: Peri<'static, DMA_CH3>,
            pub dma_ch4: Peri<'static, DMA_CH4>,
            pub dma_ch5: Peri<'static, DMA_CH5>,
            pub dma_ch6: Peri<'static, DMA_CH6>,
            pub dma_ch7: Peri<'static, DMA_CH7>,
            pub dma_ch8: Peri<'static, DMA_CH8>,
            pub dma_ch9: Peri<'static, DMA_CH9>,
            pub dma_ch10: Peri<'static, DMA_CH10>,
            pub dma_ch11: Peri<'static, DMA_CH11>,
            pub pwm_slice0: Peri<'static, PWM_SLICE0>,
            pub pwm_slice1: Peri<'static, PWM_SLICE1>,
            pub pwm_slice2: Peri<'static, PWM_SLICE2>,
            pub pwm_slice3: Peri<'static, PWM_SLICE3>,
            pub pwm_slice4: Peri<'static, PWM_SLICE4>,
            pub pwm_slice5: Peri<'static, PWM_SLICE5>,
            pub pwm_slice6: Peri<'static, PWM_SLICE6>,
            pub pwm_slice7: Peri<'static, PWM_SLICE7>,
            /// The second core (DAGON runs on core 0 only).
            pub core1: Peri<'static, CORE1>,
            /// The round display's bus and pins; [`Display::pins`] sets them up.
            pub display: Display,
        }
    };
}
spare_gpios!(spare);
/// Holds the touch controller out of reset and otherwise leaves it alone.
pub fn hold_touch(pin: Peri<'static, TouchRst>) -> Output<'static> {
    Output::new(pin, Level::High)
}

pub struct Display {
    pub spi: Peri<'static, Lcd>,
    pub clk: Peri<'static, LcdSck>,
    pub mosi: Peri<'static, LcdMosi>,
    pub dma: Peri<'static, DMA_CH0>,
    pub cs: Peri<'static, LcdCs>,
    pub dc: Peri<'static, LcdDc>,
    pub rst: Peri<'static, LcdRst>,
    pub backlight: Peri<'static, Backlight>,
}

/// The display's bus (SPI1 at [`LCD_SPI_HZ`], write only, DMA) and lines,
/// ready for the app's panel driver.
pub struct DisplayPins {
    pub spi: Spi<'static, Lcd, Async>,
    pub cs: Output<'static>,
    pub dc: Output<'static>,
    pub rst: Output<'static>,
    /// Off: switch it on once a full frame is on the panel.
    pub backlight: Output<'static>,
}

impl Display {
    pub fn pins(
        self,
        irqs: impl Binding<DMA_IRQ_0, dma::InterruptHandler<DMA_CH0>> + 'static,
    ) -> DisplayPins {
        let mut cfg = spi::Config::default();
        cfg.frequency = LCD_SPI_HZ;
        DisplayPins {
            spi: Spi::new_txonly(self.spi, self.clk, self.mosi, self.dma, irqs, cfg),
            cs: Output::new(self.cs, Level::High),
            dc: Output::new(self.dc, Level::Low),
            rst: Output::new(self.rst, Level::High),
            backlight: Output::new(self.backlight, Level::Low),
        }
    }
}

/// DAGON's I2C to other nodes (bus `child-i2c`).
pub struct SubI2c {
    pub bus: Peri<'static, ChildI2c>,
    pub sda: Peri<'static, ChildI2cSda>,
    pub scl: Peri<'static, ChildI2cScl>,
}

impl SubI2c {
    /// The controller at [`I2C_HZ`], with the DAGON mailbox link.
    pub fn link(
        self,
        irqs: impl Binding<I2C0_IRQ, i2c::InterruptHandler<I2C0>>,
    ) -> I2cLink<RpI2c<'static, I2C0>> {
        let mut cfg = i2c::Config::default();
        cfg.frequency = I2C_HZ;
        I2cLink::new(
            RpI2c(I2c::new_async(self.bus, self.scl, self.sda, irqs, cfg)),
            dagon_mcu_rp2040::RELAY_LINK_I2C0,
        )
    }
}

/// DAGON's UART to other nodes (bus `child-uart`), on a PIO (state machines
/// 0 and 1).
pub struct SubUart {
    pub pio: Peri<'static, ChildUart>,
    pub tx: Peri<'static, ChildUartTx>,
    pub rx: Peri<'static, ChildUartRx>,
}

impl SubUart {
    /// The PIO UART at [`UART_BAUD`] as a byte stream.
    pub fn stream(
        self,
        irqs: impl Binding<PIO0_IRQ_0, pio::InterruptHandler<PIO0>>,
    ) -> PioStream<'static, PIO0, 0, 1> {
        let Pio {
            mut common,
            sm0,
            sm1,
            ..
        } = Pio::new(self.pio, irqs);
        let tx_prog = PioUartTxProgram::new(&mut common);
        let rx_prog = PioUartRxProgram::new(&mut common);
        let tx = PioUartTx::new(UART_BAUD, &mut common, sm0, self.tx, &tx_prog);
        let rx = PioUartRx::new(UART_BAUD, &mut common, sm1, self.rx, &rx_prog);
        // The UART runs for the life of the app: never release the PIO.
        core::mem::forget(common);
        PioStream { tx, rx }
    }
}

/// Reset and boot-mode lines of an STM32 node (ROM recovery path).
pub struct SubCtl {
    pub nrst: Peri<'static, ChildNrst>,
    pub boot0: Peri<'static, ChildBoot0>,
}

impl SubCtl {
    pub fn lines(self) -> SubLines<'static> {
        SubLines::new(self.nrst, self.boot0)
    }
}
