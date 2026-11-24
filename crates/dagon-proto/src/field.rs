//! The field grammar of every payload, its one reader and its one writer.
//!
//! ```text
//! request   core || field* [|| 0x00 || bulk]
//! response  status u8 || core || field* [|| 0x00 || bulk]   (non-OK: status || field*)
//! field     key || len || value        key = number * 2 + must
//! ```
//!
//! `key` and `len` are LEB128 of 1 to 4 bytes in their shortest form.
//! Field numbers rise strictly through a message, so no number repeats and
//! a message has one encoding; number 0 is never a field. Key 0 is the
//! delimiter before the bulk: a message that carries bulk always has it,
//! and a message that carries none never does. A receiver skips a field it
//! does not know when `must` is 0 and refuses the message when it is 1.
//!
//! No allocation, no recursion and no panic path: every access is checked.

use crate::{FrameError, Status};

/// Largest key or length: four LEB128 bytes.
pub const VARINT_MAX: u32 = 0x0FFF_FFFF;

/// Most fields one message of the table has; the reader holds that many.
pub const MAX_FIELDS: usize = 8;

/// One known field of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// 1 to 31 belong to the command, 32 to 63 mean the same in every
    /// command ([`crate::table::COMMON`]), 64 and above take a two-byte
    /// key. A number is never given to another field.
    pub num: u16,
    /// A receiver that does not know the field refuses the message.
    pub must: bool,
    /// Shortest and longest value, in bytes.
    pub min: u8,
    pub max: u8,
    /// Element width of a packed array (the length is a multiple of it);
    /// 1 for anything else. Another element layout is another number.
    pub elem: u8,
}

impl FieldSpec {
    pub const fn must(num: u16, min: u8, max: u8) -> Self {
        FieldSpec {
            num,
            must: true,
            min,
            max,
            elem: 1,
        }
    }

    pub const fn opt(num: u16, min: u8, max: u8) -> Self {
        FieldSpec {
            num,
            must: false,
            min,
            max,
            elem: 1,
        }
    }

    /// A packed array of `elem`-byte elements.
    pub const fn packed(self, elem: u8) -> Self {
        FieldSpec { elem, ..self }
    }

    /// Whether a value of `len` bytes is one this field takes. Counted
    /// down instead of divided: a length is at most 255 and the cores
    /// this runs on have no divider.
    fn takes(&self, len: usize) -> bool {
        if len < usize::from(self.min) || len > usize::from(self.max) {
            return false;
        }
        let elem = usize::from(self.elem);
        let mut left = len;
        while elem > 1 && left >= elem {
            left -= elem;
        }
        elem <= 1 || left == 0
    }
}

/// One message of a command: the fixed bytes it starts with, the fields it
/// knows (rising by number) and whether bulk follows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsgSpec {
    pub core: u8,
    pub fields: &'static [FieldSpec],
    pub bulk: bool,
}

/// Why the reader refused a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Malformed: `Status::Arg`.
    Arg,
    /// A must field of this number is not known: `Status::Field`.
    Field(u32),
}

impl From<Refusal> for Status {
    fn from(r: Refusal) -> Status {
        match r {
            Refusal::Arg => Status::Arg,
            Refusal::Field(_) => Status::Field,
        }
    }
}

/// A message the reader accepted: lengths, order and must bits hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parsed<'a> {
    pub core: &'a [u8],
    /// Empty for a message without bulk.
    pub bulk: &'a [u8],
    spec: &'a [FieldSpec],
    values: [Option<&'a [u8]>; MAX_FIELDS],
}

impl<'a> Parsed<'a> {
    /// The value of a field of the message's spec, when it was sent. One
    /// copy for every reader: inlined, each lookup costs flash.
    #[inline(never)]
    pub fn field(&self, f: &FieldSpec) -> Option<&'a [u8]> {
        let i = self.spec.iter().position(|s| s.num == f.num)?;
        *self.values.get(i)?
    }
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], Refusal> {
    let head = rest.get(..n).ok_or(Refusal::Arg)?;
    *rest = rest.get(n..).ok_or(Refusal::Arg)?;
    Ok(head)
}

/// LEB128, 1 to 4 bytes, shortest form only.
fn varint(rest: &mut &[u8]) -> Result<u32, Refusal> {
    let mut v = 0u32;
    for i in 0..4 {
        let b = *take(rest, 1)?.first().ok_or(Refusal::Arg)?;
        v |= u32::from(b & 0x7F) << (7 * i);
        if b < 0x80 {
            return if b == 0 && i != 0 {
                Err(Refusal::Arg)
            } else {
                Ok(v)
            };
        }
    }
    Err(Refusal::Arg)
}

/// The one reader of every payload (a response's after its status byte).
/// `Arg` for a short core, a key or length that is not the shortest form
/// or runs to a fifth byte, a number that does not rise, a length past the
/// end, a known field whose must bit or length is not the table's, a
/// missing delimiter before bulk and a delimiter where no bulk follows;
/// `Field` for an unknown must field. An unknown optional field is skipped.
pub fn read<'a>(payload: &'a [u8], spec: &'a MsgSpec) -> Result<Parsed<'a>, Refusal> {
    let mut rest = payload;
    let mut m = Parsed {
        core: take(&mut rest, usize::from(spec.core))?,
        bulk: &[],
        spec: spec.fields,
        values: [None; MAX_FIELDS],
    };
    let mut prev = 0;
    loop {
        if rest.is_empty() {
            return if spec.bulk { Err(Refusal::Arg) } else { Ok(m) };
        }
        let key = varint(&mut rest)?;
        if key == 0 {
            m.bulk = rest;
            return if spec.bulk { Ok(m) } else { Err(Refusal::Arg) };
        }
        let (num, must) = (key >> 1, key & 1 != 0);
        if num <= prev {
            return Err(Refusal::Arg);
        }
        prev = num;
        let len = varint(&mut rest)? as usize;
        let value = take(&mut rest, len)?;
        let known = spec
            .fields
            .iter()
            .enumerate()
            .find(|(_, s)| u32::from(s.num) == num);
        match known {
            Some((i, s)) if s.must == must && s.takes(len) => {
                *m.values.get_mut(i).ok_or(Refusal::Arg)? = Some(value);
            }
            Some(_) => return Err(Refusal::Arg),
            None if must => return Err(Refusal::Field(num)),
            None => {}
        }
    }
}

/// The LEB128 bytes of `v` in their shortest form and how many they are.
pub(crate) fn shortest(v: u32) -> Option<([u8; 4], usize)> {
    if v > VARINT_MAX {
        return None;
    }
    let mut b = [0u8; 4];
    let (mut v, mut n) = (v, 0);
    loop {
        let slot = b.get_mut(n)?;
        n += 1;
        if v < 0x80 {
            *slot = v as u8;
            return Some((b, n));
        }
        *slot = v as u8 | 0x80;
        v >>= 7;
    }
}

/// The value of a canonical varint that fills `b`.
pub(crate) fn shortest_value(b: &[u8]) -> Option<u32> {
    let mut rest = b;
    let v = varint(&mut rest).ok()?;
    rest.is_empty().then_some(v)
}

/// The one writer of every payload: the core, then fields in rising
/// number order, then the bulk. A step that would not fit, a number that
/// is 0 or does not rise and a value outside the field's lengths are
/// refused and leave the message as it was.
pub struct FieldWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
    prev: u16,
}

impl<'a> FieldWriter<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        FieldWriter {
            buf,
            pos: 0,
            prev: 0,
        }
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.pos == 0
    }

    fn raw(&mut self, parts: &[&[u8]]) -> Result<(), FrameError> {
        let mut at = self.pos;
        for p in parts {
            let end = at.checked_add(p.len()).ok_or(FrameError::BufferTooSmall)?;
            self.buf
                .get_mut(at..end)
                .ok_or(FrameError::BufferTooSmall)?
                .copy_from_slice(p);
            at = end;
        }
        self.pos = at;
        Ok(())
    }

    /// Core bytes; before any field.
    pub fn put(&mut self, core: &[u8]) -> Result<&mut Self, FrameError> {
        if self.prev != 0 {
            return Err(FrameError::Field);
        }
        self.raw(&[core]).map(|()| self)
    }

    pub fn field(&mut self, f: &FieldSpec, value: &[u8]) -> Result<&mut Self, FrameError> {
        if f.num <= self.prev || !f.takes(value.len()) {
            return Err(FrameError::Field);
        }
        let key = shortest(u32::from(f.num) * 2 + u32::from(f.must));
        let len = shortest(value.len() as u32);
        let (Some((k, kn)), Some((l, ln))) = (key, len) else {
            return Err(FrameError::Field);
        };
        let key = k.get(..kn).ok_or(FrameError::Field)?;
        let len = l.get(..ln).ok_or(FrameError::Field)?;
        self.raw(&[key, len, value])?;
        self.prev = f.num;
        Ok(self)
    }

    /// [`FieldWriter::field`] unless the value is the one its absence
    /// means.
    pub fn field_unless(
        &mut self,
        f: &FieldSpec,
        value: &[u8],
        absent: &[u8],
    ) -> Result<&mut Self, FrameError> {
        if value == absent {
            return Ok(self);
        }
        self.field(f, value)
    }

    /// The delimiter and the bulk; nothing follows.
    pub fn bulk(&mut self, bulk: &[u8]) -> Result<&mut Self, FrameError> {
        self.raw(&[&[0], bulk])?;
        self.prev = u16::MAX;
        Ok(self)
    }
}
