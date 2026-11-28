//! The CPU clock stage0 checks on: the ROM runs the CPU from the 40 MHz
//! crystal divided by 2 (20 MHz), where one signature check takes over two
//! seconds. Stage0 starts the BBPLL at 480 MHz and runs the CPU from it at
//! 80 MHz, as ESP-IDF's 2nd stage does (`bootloader_clock_configure`,
//! `rtc_clk_cpu_freq_set_config`), then puts the CPU back on the crystal
//! with the ROM's divider before it starts an image, so the image finds
//! the clocks the ROM left (the UART's bit rate among them) and the BBPLL
//! running and calibrated, as an image the 2nd stage starts finds it.
//!
//! Registers, analog (regi2c) values and order as ESP-IDF v5.3
//! `esp_hw_support/port/esp32s3/rtc_clk.c`, `hal/esp32s3/clk_tree_ll.h`,
//! `hal/esp32s3/regi2c_ctrl_ll.h`, `soc/esp32s3/regi2c_bbpll.h` and
//! `soc/esp32s3/{system_reg,rtc_cntl_reg}.h`. With a crystal other than
//! 40 MHz, a CPU already on the PLL, or a calibration that does not end,
//! the CPU stays on the ROM's clock.

use crate::rom::{self, reg_read, reg_write};
use core::sync::atomic::{AtomicU32, Ordering};

const RTC_CNTL: u32 = 0x6000_8000;
/// RTC_CNTL_OPTIONS0: BB_I2C_FORCE_PD, BBPLL_I2C_FORCE_PD, BBPLL_FORCE_PD.
const OPTIONS0: u32 = RTC_CNTL;
const BBPLL_PD: u32 = 1 << 6 | 1 << 8 | 1 << 10;
/// RTC_CNTL_STORE4: the crystal's MHz, twice (low and high half), the low
/// bit the ROM log switch (`clk_ll_xtal_load_freq_mhz`).
const STORE4: u32 = RTC_CNTL + 0xC0;
/// RTC_CNTL_DATE: SLAVE_PD (bits 13 to 18), the LDO slaves powered down.
const DATE: u32 = RTC_CNTL + 0x1FC;
const SLAVE_PD_SHIFT: u32 = 13;
const SLAVE_PD_MASK: u32 = 0x3F << SLAVE_PD_SHIFT;
/// DEFAULT_LDO_SLAVE: at 80 MHz one more slave runs (`>> 1`).
const LDO_SLAVE: u32 = 0x7;

const SYSTEM: u32 = 0x600C_0000;
/// SYSTEM_CPU_PER_CONF: PLL_FREQ_SEL (bit 2, 1 = 480 MHz), CPUPERIOD_SEL
/// (bits 0 and 1, 0 = 80 MHz).
const CPU_PER_CONF: u32 = SYSTEM + 0x10;
const PLL_480M: u32 = 1 << 2;
const CPUPERIOD_MASK: u32 = 0b11;
/// SYSTEM_SYSCLK_CONF: SOC_CLK_SEL (bits 10 and 11; 0 crystal, 1 PLL),
/// PRE_DIV_CNT (bits 0 to 9, the crystal's divider less 1).
const SYSCLK_CONF: u32 = SYSTEM + 0x60;
const SOC_CLK_MASK: u32 = 0b11 << 10;
const SOC_CLK_PLL: u32 = 1 << 10;
const PRE_DIV_MASK: u32 = 0x3FF;

/// I2C_MST_ANA_CONF0: BBPLL_STOP_FORCE_HIGH (bit 2), _LOW (bit 3) and
/// BBPLL_CAL_DONE (bit 24).
const ANA_CONF0: u32 = 0x6000_E040;
const STOP_HIGH: u32 = 1 << 2;
const STOP_LOW: u32 = 1 << 3;
const CAL_DONE: u32 = 1 << 24;
/// ANA_CONFIG: the regi2c slaves' gates (bits 8 to 17; set gates one
/// off), the BBPLL's bit 17.
const ANA_CONFIG: u32 = 0x6000_E044;
const ANA_GATES: u32 = 0x3FF << 8;
const ANA_BBPLL: u32 = 1 << 17;

/// The BBPLL's regi2c block and host.
const BBPLL: u8 = 0x66;
const HOST: u8 = 1;

/// The crystal the 480 MHz values below are for.
const XTAL_MHZ: u32 = 40;
/// The CPU clock stage0 checks on.
pub const MHZ: u32 = 80;
/// Polls of CAL_DONE before stage0 keeps the ROM's clock (the calibration
/// takes microseconds; this is over 50 ms at 20 MHz).
const CAL_POLLS: u32 = 200_000;

/// SYSCLK_CONF, the ROM's MHz and the LDO slaves as the ROM left them; the
/// MHz 0 while the CPU is on the ROM's clock.
static ROM_SYSCLK: AtomicU32 = AtomicU32::new(0);
static ROM_MHZ: AtomicU32 = AtomicU32::new(0);
static ROM_SLAVE: AtomicU32 = AtomicU32::new(0);

fn modify(reg: u32, clear: u32, set: u32) {
    reg_write(reg, reg_read(reg) & !clear | set);
}

fn xtal_mhz() -> Option<u32> {
    let r = reg_read(STORE4);
    let (lo, hi) = (r & 0xFFFF, r >> 16);
    (lo == hi && r != 0 && r != u32::MAX).then_some(lo & !1)
}

/// Starts the BBPLL at 480 MHz and calibrates it; false when the
/// calibration does not end.
fn start_bbpll() -> bool {
    // The regi2c slaves gated off, then the BBPLL's opened
    // (`regi2c_ctrl_ll_i2c_reset`, `_i2c_bbpll_enable`).
    modify(ANA_CONFIG, 0, ANA_GATES);
    modify(ANA_CONFIG, ANA_BBPLL, 0);
    // Powered (`clk_ll_bbpll_enable`), its digital part at 480 MHz.
    modify(OPTIONS0, BBPLL_PD, 0);
    modify(CPU_PER_CONF, 0, PLL_480M);
    modify(ANA_CONF0, STOP_HIGH, STOP_LOW);
    // `clk_ll_bbpll_set_config(480, 40)`: div_ref 0, div7_0 8, dr1 0,
    // dr3 0, dchgp 5, dcur 3, dbias 3.
    // SAFETY: the ROM's regi2c writes of the BBPLL's registers.
    unsafe {
        rom::rom_i2c_writeReg(BBPLL, HOST, 4, 0x6B); // MODE_HF
        rom::rom_i2c_writeReg(BBPLL, HOST, 2, 5 << 4); // OC_REF_DIV: dchgp, div_ref
        rom::rom_i2c_writeReg(BBPLL, HOST, 3, 8); // OC_DIV_7_0
        rom::rom_i2c_writeReg_Mask(BBPLL, HOST, 5, 2, 0, 0); // OC_DR1
        rom::rom_i2c_writeReg_Mask(BBPLL, HOST, 5, 6, 4, 0); // OC_DR3
        rom::rom_i2c_writeReg(BBPLL, HOST, 6, 1 << 6 | 3 << 4 | 3); // OC_DCUR: dlref, dhref, dcur
        rom::rom_i2c_writeReg_Mask(BBPLL, HOST, 9, 1, 0, 3); // OC_VCO_DBIAS
    }
    let done = (0..CAL_POLLS).any(|_| reg_read(ANA_CONF0) & CAL_DONE != 0);
    // SAFETY: the ROM's busy wait.
    unsafe { rom::ets_delay_us(10) };
    modify(ANA_CONF0, STOP_LOW, STOP_HIGH);
    done
}

/// Runs the CPU at [`MHZ`] from the BBPLL, keeping what [`hand_back`]
/// restores; leaves the ROM's clock where the module doc says.
pub fn raise() {
    let sysclk = reg_read(SYSCLK_CONF);
    if sysclk & SOC_CLK_MASK != 0 || xtal_mhz() != Some(XTAL_MHZ) || !start_bbpll() {
        return;
    }
    // SAFETY: a ROM function without arguments.
    let rom_mhz = unsafe { rom::ets_get_cpu_frequency() }.max(1);
    let date = reg_read(DATE);
    ROM_SYSCLK.store(sysclk, Ordering::Relaxed);
    ROM_MHZ.store(rom_mhz, Ordering::Relaxed);
    ROM_SLAVE.store(date & SLAVE_PD_MASK, Ordering::Relaxed);
    // The LDO raised before the clock (`rtc_clk_cpu_freq_to_pll_mhz`).
    modify(
        DATE,
        SLAVE_PD_MASK,
        (LDO_SLAVE >> (MHZ / 80)) << SLAVE_PD_SHIFT,
    );
    modify(CPU_PER_CONF, CPUPERIOD_MASK, 0);
    modify(SYSCLK_CONF, PRE_DIV_MASK, 0);
    modify(SYSCLK_CONF, SOC_CLK_MASK, SOC_CLK_PLL);
    // SAFETY: the ROM's tick count for its delays.
    unsafe { rom::ets_update_cpu_frequency(MHZ) };
}

/// Puts the CPU back on the crystal with the ROM's divider and the ROM's
/// tick count (`rtc_clk_cpu_freq_to_xtal`), the BBPLL left running; does
/// nothing when [`raise`] kept the ROM's clock.
pub fn hand_back() {
    let rom_mhz = ROM_MHZ.load(Ordering::Relaxed);
    if rom_mhz == 0 {
        return;
    }
    let sysclk = ROM_SYSCLK.load(Ordering::Relaxed);
    // SAFETY: the ROM's tick count for its delays.
    unsafe { rom::ets_update_cpu_frequency(rom_mhz) };
    // Divider 1 first, then the ROM's, then the crystal.
    modify(SYSCLK_CONF, PRE_DIV_MASK, 0);
    modify(SYSCLK_CONF, PRE_DIV_MASK, sysclk & PRE_DIV_MASK);
    modify(SYSCLK_CONF, SOC_CLK_MASK, 0);
    modify(DATE, SLAVE_PD_MASK, ROM_SLAVE.load(Ordering::Relaxed));
}
