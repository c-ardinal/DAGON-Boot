# Golden vectors

Byte-exact reference files for the DAGON formats and link frames. Tests
rebuild every file from fixed inputs, require an exact match, check the field
offsets against a hand-written table, then parse and verify the file:

- `crates/dagon-format/tests/golden.rs` (formats)
- `crates/dagon-proto/tests/golden_frames.rs` (frames)

Regenerate only on an intentional format change:
`DAGON_REGEN_GOLDEN=1 cargo test -p dagon-format -p dagon-proto`.

Signatures use the RFC 8032 section 7.1 "TEST 1" Ed25519 key (public test
data; images carry the `TEST_KEY` build flag). Public key:
`d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a`.

| File | Content |
|---|---|
| `image_f401_app_a.img` | Signed F401 APP image for slot A: 512-byte header (version 1.1.0 as `0x01010000` LE at 0x018, inline, `seg-f401-7seg`) + 1024-byte body |
| `manifest.bin` | Signed manifest, 2 entries (F401 APP A and B), no constraints, `topology_sha256` = SHA-256 of `topology_sample.bin` |
| `release.dgb` | Bundle: manifest, both F401 images (`dest = 2`), topology item (`dest = 3`, `entry_index = 0xFF`, length = topology length, CRC-32 as for every item) holding `topology_sample.bin` |
| `topology_sample.bin` | Topology canonical binary of the sample (160 B): hub RP2040 coordinator; seg F401 on I2C0 SDA/SCL GPIO16/17 at 400 kHz, address 0x42, NRST GPIO18, BOOT0 GPIO28, STM32 I2C ROM entry; watch ESP32-S3 on PIO UART TX/RX GPIO26/27 at 115200, cooperative; update order seg, watch, hub |
| `statelog_face.bin` | State log face: header (generation 1), PROVISION A, CONFIRMED A, SLOT_PENDING B, BOOT_ATTEMPT B, ROLLBACK to A (TRIAL_EXHAUSTED), one erased record |
| `frame_hello_req.bin` | HELLO request (proto_ver 1, max_payload 1088), seq 0x01 |
| `frame_hello_resp.bin` | HELLO response: OK + 24-byte body (node, kind S, F401, app mode, 1.1.0, trial, max_payload 256) |
| `frame_status_resp.bin` | STATUS response: OK + 71-byte body (A written 1.0.0, B trying 1.1.0, factory empty) |
| `frame_xfer_begin.bin` | XFER_BEGIN APP slot B, total_len 1536, hdr_len 512 |
| `frame_xfer_hdr.bin` | XFER_HDR offset 252 with 252 bytes: a maximal 268-byte I2C frame |
| `frame_relay_hello.bin` | RELAY on I2C0 to 0x42, timeout 300 ms, wrapping `frame_hello_req.bin` |
| `frame_show.bin` | SHOW node 1, phase TRANSFER, 42 %, 1.1.0, text `seg` |
| `frame_subctl.bin` | SUBCTL nrst = pulse, boot0 = high |
| `frame_i2c_raw_get.bin` | I2C_RAW writing `00 FF`, reading 1 byte |
| `frame_enter_rom.bin` | ENTER_ROM (0x24) method = 1 FORCE_DOWNLOAD_BOOT, seq 0x14. Added later as an additive change: the files above are unchanged |
| `frame_version_req.bin` | VERSION (0x03) request, empty (the summary set), seq 0x15. Additive: the files above are unchanged |
| `frame_version_resp.bin` | VERSION response: OK, 6 TLVs: wire 1, mcu_id F401, stage0 0.1.0, running boot core 1.0.0, boot core slots (A confirmed 1.0.0 running, B empty), app slots (A written 1.0.0, B trying 1.1.0 running, factory empty) |
