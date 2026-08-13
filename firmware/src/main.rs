// Author: Cursor Grok 4.6
// Purpose: ESP32-C6 firmware binary stub; Wi-Fi/iperf land in WP2.

#![no_std]
#![no_main]

// Wi‑Fi/iperf land in WP2.

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
