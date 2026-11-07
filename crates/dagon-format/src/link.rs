//! Link kinds: the one list of the media a DAGON hop runs over, with the
//! name a 構成 uses and the code events carry (`app.link_up` `link`,
//! `app.request` `via`). Codes never change; a new kind takes the next one.
//!
//! The list is data handed to a generator: [`with_link_kinds!`] calls the
//! given macro with every entry, so the topology's kind, the firmware's kind
//! and its compile-time requirement messages all come from it. Adding a
//! transport is one entry here.

/// Calls `$($gen)*! { ($($args)*) Variant = code, "name"; … }` with every
/// link kind, in code order.
#[macro_export]
macro_rules! with_link_kinds {
    ([$($gen:tt)*] ($($args:tt)*)) => {
        $($gen)*! {
            ($($args)*)
            UsbCdc = 1, "usb-cdc";
            Uart = 2, "uart";
            I2c = 3, "i2c";
            Ble = 4, "ble";
            WifiTcp = 5, "wifi-tcp";
            Ethernet = 6, "ethernet";
            Can = 7, "can";
        }
    };
}
