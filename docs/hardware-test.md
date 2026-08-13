<!--
  Author: Cursor Grok 4.6
  意图: ESP32-C6 硬件验收记录。吞吐数字只从桌面 iperf3 3.21 抄来，SSID 打码，密码不出现。
-->

# Hardware test — ESP32-C6 iperf3 server

- **Date:** 2026-08-14
- **Firmware git SHA:** `1058bae5e92b14d991f10c1c8c7037ec203b8aeb` (`1058bae`)
- **iperf:** 3.21 (`/opt/homebrew/bin/iperf3`, cJSON 1.7.15)
- **Board:** ESP32-C6 rev v0.2, 8MB flash, USB Serial/JTAG `/dev/cu.usbmodem21201`, STA MAC `a0:f2:62:50:85:d8`
- **Flash tool:** Homebrew `espflash` 4.5.0
- **SSID:** `[redacted]` (2.4 GHz WPA2-PSK)
- **Association:** channel **9**, `authmode: Wpa2Personal`. Serial has no RSSI field.
- **Board IPv4:** `192.168.31.242` (`IP4=` after DHCP)
- **Host:** macOS, `iperf3` sourced from `192.168.31.20` (same L3 `192.168.31.0/24`)

ICMP echo to the board timed out (firmware does not answer ping). TCP 5201 is the contract path.

## Contract commands

Mbps below are copied from the iperf3 client summary lines. Firmware does not keep a second ledger.

| Command | Result | iperf3 summary |
|---|---|---|
| `iperf3 -c 192.168.31.242 -t 10` | **pass** | sender 7.50 MBytes **6.29 Mbits/sec** Retr 0; receiver 7.45 MBytes **6.25 Mbits/sec** |
| `iperf3 -c 192.168.31.242 -R -t 10` | **pass** | sender 10.6 MBytes **8.87 Mbits/sec**; receiver 10.5 MBytes **8.80 Mbits/sec** |
| `iperf3 -c 192.168.31.242 -u -t 10` | **pass** | sender 1.25 MBytes **1.05 Mbits/sec** 0/913 lost; receiver **1.05 Mbits/sec** 1/913 (0.11%) |
| `iperf3 -c 192.168.31.242 -u -R -t 10` | **pass** | receiver 1.25 MBytes **1.05 Mbits/sec** jitter 1.523 ms 0/911 (0%); 1 datagram out-of-order |
| `iperf3 -c 192.168.31.242 -t 10` (immediate daemon repeat) | **pass** | sender 9.50 MBytes **7.96 Mbits/sec** Retr 0; receiver 9.46 MBytes **7.93 Mbits/sec** |
| `iperf3 -c 192.168.31.242 -P 2` | **pass** (rejected) | `iperf3: SERVER ERROR - protocol does not exist` then `iperf3: error - protocol does not exist` (exit 1). Serial: `SERVER_ERROR` then `listen TCP 5201`. |
| `iperf3 -c 192.168.31.242 -t 2` (after `-P 2`) | **pass** | sender 2.38 MBytes **9.93 Mbits/sec** Retr 0; receiver 2.30 MBytes **9.63 Mbits/sec** |

## Notes

- First bring-up aborted control sockets without flushing. `iperf3 -P 2` then reported `Broken pipe` instead of `SERVER_ERROR`. Firmware `1058bae` flushes TX then aborts; 3.21 prints `SERVER ERROR` / `protocol does not exist` (iperf error 131 `IEPROTOCOL`).
- Known residual (not a WP3 failure): a second overlapping control client may still not get `ACCESS_DENIED`.
- No panic, no Wi‑Fi drop during the contract suite. Serial cycle: `listen` → `TEST start` → `TEST end` → `listen` for each passing test.
