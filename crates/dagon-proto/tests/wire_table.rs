//! The field grammar over every command of the table, in both directions:
//! an unknown optional field changes nothing, an unknown must field is
//! refused with its number, every malformed form is `Arg`, each core is
//! frozen byte for byte, and no input makes the reader panic.

use dagon_format::Version;
use dagon_proto::field::{read, FieldSpec, FieldWriter, MsgSpec, Refusal, MAX_FIELDS, VARINT_MAX};
use dagon_proto::payload::*;
use dagon_proto::{table, Cmd, Status};

/// A field number no command has (a two-byte key).
const UNKNOWN: u16 = 300;

/// Every message of the table: (command, is a response, its spec).
fn messages() -> Vec<(Cmd, bool, &'static MsgSpec)> {
    Cmd::ALL
        .into_iter()
        .flat_map(|c| [(c, false, &c.spec().req), (c, true, &c.spec().resp)])
        .collect()
}

/// A well-formed message of `spec`: its core, every field at its shortest
/// length, then `extra`, then the bulk.
fn sample(spec: &MsgSpec, extra: &[(FieldSpec, &[u8])]) -> Vec<u8> {
    let mut buf = [0u8; 512];
    let mut w = FieldWriter::new(&mut buf);
    w.put(&vec![0xC0; usize::from(spec.core)]).unwrap();
    for f in spec.fields {
        w.field(f, &vec![0; usize::from(f.min)]).unwrap();
    }
    for (f, v) in extra {
        w.field(f, v).unwrap();
    }
    if spec.bulk {
        w.bulk(&[0, 3, 1, 0xDA]).unwrap();
    }
    let n = w.len();
    buf[..n].to_vec()
}

/// `p` with the raw bytes `tail` where the next field would go.
fn with_tail(spec: &MsgSpec, tail: &[u8]) -> Vec<u8> {
    let mut p = sample(spec, &[]);
    let at = p.len() - if spec.bulk { 5 } else { 0 };
    p.splice(at..at, tail.iter().copied());
    p
}

#[test]
fn unknown_optional_field_changes_nothing() {
    for (cmd, resp, spec) in messages() {
        let plain = sample(spec, &[]);
        let want = read(&plain, spec).unwrap_or_else(|e| panic!("{cmd:?} {resp}: {e:?}"));
        for v in [&[][..], &[9, 9], &[0; 200]] {
            let more = sample(spec, &[(FieldSpec::opt(UNKNOWN, 0, 255), v)]);
            let got = read(&more, spec).unwrap_or_else(|e| panic!("{cmd:?} {resp}: {e:?}"));
            assert_eq!(got.core, want.core, "{cmd:?} {resp}");
            assert_eq!(got.bulk, want.bulk, "{cmd:?} {resp}");
            for f in spec.fields {
                assert_eq!(
                    got.field(f),
                    want.field(f),
                    "{cmd:?} {resp} field {}",
                    f.num
                );
            }
        }
    }
}

#[test]
fn unknown_must_field_is_refused_with_its_number() {
    for (cmd, resp, spec) in messages() {
        for (num, v) in [
            (UNKNOWN, &[][..]),
            (UNKNOWN, &[1, 2]),
            (31, &[7]),
            (8000, &[]),
        ] {
            if spec.fields.iter().any(|f| f.num >= num) {
                continue;
            }
            let p = sample(spec, &[(FieldSpec::must(num, 0, 255), v)]);
            assert_eq!(
                read(&p, spec).err(),
                Some(Refusal::Field(u32::from(num))),
                "{cmd:?} {resp} field {num}"
            );
        }
    }
}

#[test]
fn every_malformed_form_is_arg() {
    for (cmd, resp, spec) in messages() {
        let arg = |p: &[u8], why: &str| {
            assert_eq!(
                read(p, spec).err(),
                Some(Refusal::Arg),
                "{cmd:?} {resp}: {why}"
            )
        };
        // 300 * 2 = 0x258: key bytes D8 04 (optional), D9 04 (must).
        arg(&with_tail(spec, &[0xD8, 0x84, 0x00, 0]), "non-minimal key");
        arg(
            &with_tail(spec, &[0xD8, 0x04, 0x80, 0x00]),
            "non-minimal length",
        );
        arg(
            &with_tail(spec, &[0x80, 0x80, 0x80, 0x80, 0x01, 0]),
            "fifth varint byte",
        );
        arg(
            &with_tail(spec, &[0xD8, 0x04, 0, 0xD8, 0x04, 0]),
            "duplicate unknown",
        );
        arg(
            &with_tail(spec, &[0xDA, 0x04, 0, 0xD8, 0x04, 0]),
            "falling order",
        );
        arg(&with_tail(spec, &[0x01, 0]), "field number 0");
        arg(
            &with_tail(spec, &[0xD8, 0x04, 0xFF, 0x7F]),
            "length past the end",
        );
        if !spec.bulk {
            arg(&with_tail(spec, &[0xD8]), "key cut");
            arg(&with_tail(spec, &[0xD8, 0x04]), "length missing");
            arg(
                &with_tail(spec, &[0]),
                "delimiter in a message without bulk",
            );
        } else {
            let p = sample(spec, &[]);
            arg(&p[..p.len() - 5], "bulk without its delimiter");
        }
        if spec.core > 0 {
            arg(&sample(spec, &[])[..usize::from(spec.core) - 1], "core cut");
        }
        for (i, f) in spec.fields.iter().enumerate() {
            // The message up to field `i`, then that field in a wrong form.
            let head = |bad: FieldSpec, v: &[u8]| {
                let mut buf = [0u8; 600];
                let mut w = FieldWriter::new(&mut buf);
                w.put(&vec![0xC0; usize::from(spec.core)]).unwrap();
                for g in &spec.fields[..i] {
                    w.field(g, &vec![0; usize::from(g.min)]).unwrap();
                }
                w.field(&bad, v).unwrap();
                if spec.bulk {
                    w.bulk(&[]).unwrap();
                }
                let n = w.len();
                buf[..n].to_vec()
            };
            let free = |must| FieldSpec {
                must,
                min: 0,
                max: 255,
                elem: 1,
                ..*f
            };
            let ok = vec![0u8; usize::from(f.min)];
            arg(&head(free(!f.must), &ok), "must bit not the table's");
            if f.max < 255 {
                arg(
                    &head(free(f.must), &vec![0; usize::from(f.max) + 1]),
                    "known field too long",
                );
            }
            if f.min > 0 {
                arg(&head(free(f.must), &ok[1..]), "known field too short");
            }
            if f.elem > 1 && f.max > f.min {
                arg(
                    &head(free(f.must), &vec![0; usize::from(f.min) + 1]),
                    "packed array not a multiple of its element",
                );
            }
            let mut twice = head(free(f.must), &ok);
            let mut one = [0u8; 300];
            let mut w = FieldWriter::new(&mut one);
            w.field(&free(f.must), &ok).unwrap();
            let (n, at) = (w.len(), twice.len() - usize::from(spec.bulk));
            twice.splice(at..at, one[..n].iter().copied());
            arg(&twice, "duplicate known");
        }
    }
}

#[test]
fn bulk_starts_after_the_delimiter_whatever_it_holds() {
    for (cmd, resp, spec) in messages() {
        if !spec.bulk {
            continue;
        }
        let p = sample(spec, &[]);
        let got = read(&p, spec).unwrap();
        assert_eq!(got.bulk, &[0, 3, 1, 0xDA], "{cmd:?} {resp}");
        let mut none = p.clone();
        none.truncate(p.len() - 4);
        assert_eq!(read(&none, spec).unwrap().bulk, &[] as &[u8]);
    }
}

#[test]
fn writer_refuses_what_the_reader_would() {
    let mut buf = [0u8; 16];
    let mut w = FieldWriter::new(&mut buf);
    w.field(&FieldSpec::opt(2, 0, 4), &[1]).unwrap();
    assert!(
        w.field(&FieldSpec::opt(2, 0, 4), &[1]).is_err(),
        "same number"
    );
    assert!(w.field(&FieldSpec::opt(1, 0, 4), &[1]).is_err(), "falling");
    assert!(
        w.field(&FieldSpec::opt(3, 0, 4), &[0; 5]).is_err(),
        "over max"
    );
    assert!(
        w.field(&FieldSpec::opt(3, 2, 4), &[0]).is_err(),
        "under min"
    );
    assert!(
        w.field(&FieldSpec::opt(4, 0, 255), &[0; 14]).is_err(),
        "overflow"
    );
    assert_eq!(w.len(), 3);
    let mut w = FieldWriter::new(&mut buf);
    assert!(w.field(&FieldSpec::opt(0, 0, 4), &[]).is_err(), "number 0");
    assert!(
        w.field(&FieldSpec::opt(4, 0, 6).packed(3), &[0; 4])
            .is_err(),
        "not a multiple of the element"
    );
    assert_eq!(w.len(), 0);
}

#[test]
fn the_table_is_well_formed() {
    assert!(Cmd::from_u8(table::RESERVED_CMD).is_none());
    assert_eq!(table::RESERVED_CMD, 0xFF);
    let mut seen = [false; 256];
    for c in Cmd::ALL {
        assert!(!seen[c as usize], "{c:?} reuses a number");
        seen[c as usize] = true;
        assert_eq!(Cmd::from_u8(c as u8), Some(c));
    }
    let common: Vec<u16> = table::COMMON.iter().map(|f| f.num).collect();
    let all = messages();
    let specs = all
        .iter()
        .map(|m| m.2)
        .chain([&table::ERROR])
        .collect::<Vec<_>>();
    for spec in specs {
        assert!(spec.fields.len() <= MAX_FIELDS);
        assert!(spec.fields.windows(2).all(|w| w[0].num < w[1].num));
        for f in spec.fields {
            assert!(f.min <= f.max && f.elem >= 1);
            match f.num {
                1..=31 => {}
                32..=63 => assert!(common.contains(&f.num), "{} is not common", f.num),
                n => panic!("field {n}: per-command numbers are 1 to 31"),
            }
        }
    }
    assert!(table::COMMON.iter().all(|f| (32..=63).contains(&f.num)));
}

/// What a resend cannot harm: every command but the ones whose second run
/// is not the first one's answer.
#[test]
fn idempotent_column() {
    let never: Vec<Cmd> = Cmd::ALL
        .into_iter()
        .filter(|c| !c.spec().idempotent)
        .collect();
    assert_eq!(
        never,
        [
            Cmd::Reboot,
            Cmd::EnterRom,
            Cmd::SubCtl,
            Cmd::I2cRaw,
            Cmd::ConfRun
        ]
    );
}

/// One typed message of every command and direction: its bytes and a
/// typed parse printed, so the same checks run through the typed readers.
type Typed = (Vec<u8>, Box<dyn Fn(&[u8]) -> Result<String, Status>>);

fn enc(f: impl FnOnce(&mut [u8]) -> usize) -> Vec<u8> {
    let mut b = [0u8; 256];
    let n = f(&mut b);
    b[..n].to_vec()
}

fn dbg<T: std::fmt::Debug>(r: Result<T, Status>) -> Result<String, Status> {
    r.map(|v| format!("{v:?}"))
}

fn typed(cmd: Cmd, resp: bool) -> Typed {
    let empty = move || -> Typed {
        (
            vec![],
            Box::new(move |b| {
                dbg(parse_empty(
                    if resp {
                        &cmd.spec().resp
                    } else {
                        &cmd.spec().req
                    },
                    b,
                ))
            }),
        )
    };
    let v = Version::new(1, 2, 3);
    match (cmd, resp) {
        (Cmd::Hello, false) => {
            let m = HelloReq {
                proto_ver: 2,
                max_payload: 256,
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(HelloReq::parse(b))),
            )
        }
        (Cmd::Hello, true) => {
            let m = HelloResp {
                proto_ver: 2,
                role: role::NODE,
                port_kind: 1,
                mcu_id: 2,
                mode: mode::UPDATE,
                active_version: v,
                trial: true,
                key_id: [7; 8],
                max_payload: 256,
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(HelloResp::parse(b))),
            )
        }
        (Cmd::Status, false) => (
            enc(|b| encode_status_req(target::BOOTCORE, b).unwrap()),
            Box::new(|b| dbg(parse_status_req(b))),
        ),
        (Cmd::Status, true) => {
            let m = StatusResp {
                active: 1,
                confirmed: 0,
                attempts: 2,
                last_reason: 3,
                log_generation: 9,
                ..Default::default()
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(StatusResp::parse(b))),
            )
        }
        (Cmd::Version, false) => (vec![], Box::new(|b| dbg(parse_version_req(b)))),
        (Cmd::Version, true) => {
            let m = VersionResp {
                wire: Some(2),
                mcu_id: Some(3),
                stage0: Some(v),
                boot_core: Some(v),
                core_slots: Some(VersionSlots::default()),
                app_slots: Some(VersionSlots::default()),
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(VersionResp::parse(b))),
            )
        }
        (Cmd::XferBegin, false) => {
            let m = XferBegin {
                target: target::APP,
                slot: 1,
                total_len: 4096,
                hdr_len: IMAGE_HDR_LEN,
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(XferBegin::parse(b))),
            )
        }
        (Cmd::XferData | Cmd::XferHdr, false) => {
            let m = XferChunk {
                offset: 512,
                data: &[0, 1, 2],
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(XferChunk::parse(b))),
            )
        }
        (Cmd::XferData | Cmd::XferHdr, true) => (
            enc(|b| encode_next_offset(515, b).unwrap()),
            Box::new(|b| dbg(parse_next_offset(b))),
        ),
        (Cmd::XferEnd, true) => (
            enc(|b| encode_digest(&[0xAB; 32], b).unwrap()),
            Box::new(|b| dbg(parse_digest(b))),
        ),
        (Cmd::SetPending, false) => {
            let m = SetPending {
                target: target::APP,
                slot: 1,
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(SetPending::parse(b))),
            )
        }
        (Cmd::Reboot, false) => (
            enc(|b| encode_reboot(reboot_mode::UPDATE, b).unwrap()),
            Box::new(|b| dbg(parse_reboot(b))),
        ),
        (Cmd::Confirm, false) => (
            enc(|b| encode_confirm(target::APP, b).unwrap()),
            Box::new(|b| dbg(parse_confirm(b))),
        ),
        (Cmd::EnterRom, false) => {
            let m = EnterRom {
                method: rom_method::PMIC,
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(EnterRom::parse(b))),
            )
        }
        (Cmd::Show, false) => {
            let m = Show {
                node: 2,
                phase: phase::TRANSFER,
                percent: 40,
                version: v,
                text: b"seg",
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(Show::parse(b))),
            )
        }
        (Cmd::Relay, false) => {
            let m = Relay {
                link: 1,
                addr: 0x42,
                timeout_ms: Some(300),
                inner: &[0xDA, 0x6F, 0, 9],
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(Relay::parse(b))),
            )
        }
        (Cmd::Relay | Cmd::I2cRaw, true) => (
            enc(|b| encode_bulk_resp(&[0, 0xDA, 0x6F], b).unwrap()),
            Box::new(move |b| dbg(parse_bulk_resp(cmd, b))),
        ),
        (Cmd::SubCtl, false) => {
            let m = SubCtl {
                nrst: nrst::PULSE,
                boot0: boot0::HIGH,
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(SubCtl::parse(b))),
            )
        }
        (Cmd::I2cRaw, false) => {
            let m = I2cRaw {
                read_len: 3,
                data: &[0, 0xFF],
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(I2cRaw::parse(b))),
            )
        }
        (Cmd::ConfRun, false) => {
            let m = ConfRunReq {
                test_id: 4,
                param: 77,
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(ConfRunReq::parse(b))),
            )
        }
        (Cmd::ConfRun, true) => {
            let m = ConfRunResp {
                verdict: verdict::PASS,
                measured: 12,
                detail: b"ok",
            };
            (
                enc(|b| m.encode(b).unwrap()),
                Box::new(|b| dbg(ConfRunResp::parse(b))),
            )
        }
        (
            Cmd::Stay
            | Cmd::XferEnd
            | Cmd::XferBegin
            | Cmd::SetPending
            | Cmd::Reboot
            | Cmd::Confirm
            | Cmd::EnterRom
            | Cmd::Show
            | Cmd::SubCtl,
            _,
        ) => empty(),
    }
}

/// `p` with one more field (`key`, zero length... `value`) after its last.
fn appended(p: &[u8], spec: &MsgSpec, field: &[u8]) -> Vec<u8> {
    let bulk = read(p, spec).unwrap().bulk.len();
    let at = p.len() - if spec.bulk { bulk + 1 } else { 0 };
    let mut out = p.to_vec();
    out.splice(at..at, field.iter().copied());
    out
}

#[test]
fn typed_readers_follow_the_same_rules() {
    for (cmd, resp, spec) in messages() {
        let (bytes, parse) = typed(cmd, resp);
        let want = parse(&bytes).unwrap_or_else(|e| panic!("{cmd:?} {resp}: {e:?}"));
        assert_eq!(
            parse(&appended(&bytes, spec, &[0xD8, 0x04, 2, 9, 9])).as_ref(),
            Ok(&want),
            "{cmd:?} {resp}: unknown optional field"
        );
        assert_eq!(
            parse(&appended(&bytes, spec, &[0xD9, 0x04, 0])),
            Err(Status::Field),
            "{cmd:?} {resp}: unknown must field"
        );
        assert_eq!(
            parse(&appended(&bytes, spec, &[0x01, 0])),
            Err(Status::Arg),
            "{cmd:?} {resp}: field number 0"
        );
        if !spec.bulk {
            let mut cut = bytes.clone();
            cut.push(0xD8);
            assert_eq!(parse(&cut), Err(Status::Arg), "{cmd:?} {resp}: key cut");
        }
        // The refusal a node answers with names the field.
        if !resp {
            let mut out = [0u8; 16];
            let p = appended(&bytes, spec, &[0xD9, 0x04, 0]);
            let n = put_refusal(&mut out, Status::Field, cmd as u8, &p);
            assert_eq!(&out[..n], &[Status::Field as u8, 64, 2, 0xAC, 0x02]);
            assert_eq!(refused_field(&out[1..n]), Some(300));
            let n = put_refusal(&mut out, Status::State, cmd as u8, &p);
            assert_eq!(&out[..n], &[Status::State as u8]);
        }
    }
}

/// The core of every message and the whole of the typed sample, byte for
/// byte: a core never changes, and a field keeps its number.
#[test]
fn golden_cores_and_samples() {
    #[rustfmt::skip]
    let golden: &[(Cmd, bool, u8, &str)] = &[
        (Cmd::Hello, false, 6, "02000000 0001"),
        (Cmd::Hello, true, 6, "02000000 0001 020101 040101 06020200 080101 0A0403000201 0C0101 0E080707070707070707"),
        (Cmd::Status, false, 0, "030101"),
        (Cmd::Status, true, 0, concat!(
            "023F",
            "000000000000000000000000000000000000000000",
            "000000000000000000000000000000000000000000",
            "000000000000000000000000000000000000000000",
            " 040101 060100 080102 0A0103 0C0409000000")),
        (Cmd::Version, false, 0, ""),
        (Cmd::Version, true, 0, "020403000201 0C020300 140403000201 160B0000000000000000000000 181000000000000000000000000000000000 1A0102"),
        (Cmd::Stay, false, 0, ""),
        (Cmd::Stay, true, 0, ""),
        (Cmd::XferBegin, false, 5, "00 00100000 030101 05020002"),
        (Cmd::XferBegin, true, 0, ""),
        (Cmd::XferData, false, 4, "00020000 00 000102"),
        (Cmd::XferData, true, 4, "03020000"),
        (Cmd::XferEnd, false, 0, ""),
        (Cmd::XferEnd, true, 0, "0220<digest>"),
        (Cmd::XferHdr, false, 4, "00020000 00 000102"),
        (Cmd::XferHdr, true, 4, "03020000"),
        (Cmd::SetPending, false, 1, "00 030101"),
        (Cmd::SetPending, true, 0, ""),
        (Cmd::Reboot, false, 0, "030101"),
        (Cmd::Reboot, true, 0, ""),
        (Cmd::Confirm, false, 0, ""),
        (Cmd::Confirm, true, 0, ""),
        (Cmd::EnterRom, false, 1, "02"),
        (Cmd::EnterRom, true, 0, ""),
        (Cmd::Show, false, 1, "01 020102 040128 060403000201 0803736567"),
        (Cmd::Show, true, 0, ""),
        (Cmd::Relay, false, 1, "01 030142 04022C01 00 DA6F0009"),
        (Cmd::Relay, true, 0, "00 00DA6F"),
        (Cmd::SubCtl, false, 0, "030102 050101"),
        (Cmd::SubCtl, true, 0, ""),
        (Cmd::I2cRaw, false, 0, "03020300 00 00FF"),
        (Cmd::I2cRaw, true, 0, "00 00DA6F"),
        (Cmd::ConfRun, false, 1, "04 03044D000000"),
        (Cmd::ConfRun, true, 1, "01 02040C000000 04026F6B"),
    ];
    let listed: Vec<(Cmd, bool)> = golden.iter().map(|g| (g.0, g.1)).collect();
    let all: Vec<(Cmd, bool)> = messages().iter().map(|m| (m.0, m.1)).collect();
    assert_eq!(listed, all, "one golden row per message, in table order");
    for (cmd, resp, core, hex) in golden {
        let spec = if *resp {
            &cmd.spec().resp
        } else {
            &cmd.spec().req
        };
        assert_eq!(spec.core, *core, "{cmd:?} {resp}: core length");
        let hex: String = hex
            .replace("<digest>", &"AB".repeat(32))
            .split_whitespace()
            .collect();
        let want: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        assert_eq!(typed(*cmd, *resp).0, want, "{cmd:?} {resp}");
    }
}

/// xorshift32: the fuzz is the same run every time.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

#[test]
fn reader_never_panics() {
    let mut rng = Rng(0x570_570);
    let all = messages();
    for round in 0..200_000u32 {
        let (cmd, resp, spec) = all[rng.next() as usize % all.len()];
        let mut p = if round % 2 == 0 {
            // A valid message with bytes flipped, cut or grown.
            let mut p = typed(cmd, resp).0;
            for _ in 0..rng.next() % 4 {
                if !p.is_empty() {
                    let at = rng.next() as usize % p.len();
                    p[at] = rng.next() as u8;
                }
            }
            p
        } else {
            (0..rng.next() % 40).map(|_| rng.next() as u8).collect()
        };
        match rng.next() % 4 {
            0 => p.truncate(rng.next() as usize % (p.len() + 1)),
            1 => p.extend((0..rng.next() % 6).map(|_| rng.next() as u8)),
            _ => {}
        }
        if let Ok(m) = read(&p, spec) {
            assert_eq!(m.core.len(), usize::from(spec.core));
            assert!(m.bulk.len() <= p.len());
            assert!(spec.bulk || m.bulk.is_empty());
        }
        let _ = typed(cmd, resp).1(&p);
        let mut out = [0u8; 8];
        let _ = put_refusal(&mut out, Status::Field, cmd as u8, &p);
    }
    assert_eq!(VARINT_MAX, 0x0FFF_FFFF);
}
