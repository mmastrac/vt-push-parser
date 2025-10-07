//! Event types.
use std::iter::Map;

use smallvec::SmallVec;

use crate::AsciiControl;

/// Helper function to format UTF-8 chunks with ASCII control character handling
fn fmt_utf8_bytes_with_ascii_control(
    f: &mut std::fmt::Formatter<'_>,
    bytes: &[u8],
) -> std::fmt::Result {
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if let Ok(c) = AsciiControl::try_from(c) {
                write!(f, "{c}")?;
            } else {
                write!(f, "{c}")?;
            }
        }
        if !chunk.invalid().is_empty() {
            write!(f, "<{}>", hex::encode(chunk.invalid()))?;
        }
    }
    Ok(())
}

/// Helper function to format UTF-8 chunks for parameters (simple formatting)
fn fmt_utf8_bytes_simple(f: &mut std::fmt::Formatter<'_>, bytes: &[u8]) -> std::fmt::Result {
    for chunk in bytes.utf8_chunks() {
        write!(f, "{}", chunk.valid())?;
        if !chunk.invalid().is_empty() {
            write!(f, "<{}>", hex::encode(chunk.invalid()))?;
        }
    }
    Ok(())
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VTIntermediate {
    pub(crate) data: [u8; 2],
}

impl VTIntermediate {
    pub const fn empty() -> Self {
        Self { data: [0, 0] }
    }

    pub const fn one(c: u8) -> Self {
        assert!(c >= 0x20 && c <= 0x2F);
        Self { data: [c, 0] }
    }

    pub const fn two(c1: u8, c2: u8) -> Self {
        assert!(c1 >= 0x20 && c1 <= 0x2F);
        assert!(c2 >= 0x20 && c2 <= 0x2F);
        Self { data: [c1, c2] }
    }

    pub fn has(&self, c: u8) -> bool {
        self.data[0] == c || self.data[1] == c
    }

    pub fn clear(&mut self) {
        self.data[0] = 0;
        self.data[1] = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.data[0] == 0 && self.data[1] == 0
    }

    pub fn len(&self) -> usize {
        self.data.iter().filter(|&&c| c != 0).count()
    }

    pub fn first(&self) -> Option<u8> {
        if self.data[0] != 0 {
            Some(self.data[0])
        } else {
            None
        }
    }

    pub fn second(&self) -> Option<u8> {
        if self.data[1] != 0 {
            Some(self.data[1])
        } else {
            None
        }
    }

    #[must_use]
    pub fn push(&mut self, c: u8) -> bool {
        if !(0x20..=0x2F).contains(&c) {
            return false;
        }

        // Invalid duplicate intermediate
        if self.data[0] == c {
            return false;
        }

        if self.data[0] == 0 {
            self.data[0] = c;
            true
        } else if self.data[1] == 0 {
            self.data[1] = c;
            true
        } else {
            false
        }
    }

    pub const fn const_eq(&self, other: &Self) -> bool {
        self.data[0] == other.data[0] && self.data[1] == other.data[1]
    }

    pub fn byte_len(&self) -> usize {
        self.data.iter().filter(|&&c| c != 0).count()
    }
}

impl AsRef<[u8]> for VTIntermediate {
    fn as_ref(&self) -> &[u8] {
        &self.data[..self.byte_len()]
    }
}

impl std::fmt::Debug for VTIntermediate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Inefficient
        write!(f, "'")?;
        for c in self.data.iter() {
            if *c == 0 {
                break;
            }
            write!(f, "{}", *c as char)?;
        }
        write!(f, "'")?;
        Ok(())
    }
}

pub(crate) type Param = SmallVec<[u8; 32]>;
pub(crate) type Params = SmallVec<[Param; 8]>;

static EMPTY_PARAMS: Params = Params::new_const();

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ParamBuf<'a> {
    pub(crate) params: &'a Params,
}

impl<'a> IntoIterator for ParamBuf<'a> {
    type Item = &'a [u8];
    type IntoIter = Map<std::slice::Iter<'a, Param>, fn(&Param) -> &[u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p.as_slice())
    }
}

impl<'a> IntoIterator for &ParamBuf<'a> {
    type Item = &'a [u8];
    type IntoIter = Map<std::slice::Iter<'a, Param>, fn(&Param) -> &[u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p.as_slice())
    }
}

impl<'a> ParamBuf<'a> {
    pub const fn empty() -> Self {
        ParamBuf {
            params: &EMPTY_PARAMS,
        }
    }

    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&[u8]> {
        self.params.get(index).map(|p| p.as_slice())
    }

    pub fn try_parse<T: std::str::FromStr>(&self, index: usize) -> Option<T> {
        self.params.get(index).and_then(|p| {
            std::str::from_utf8(p.as_slice())
                .ok()
                .and_then(|s| s.parse::<T>().ok())
        })
    }

    pub fn to_owned(&self) -> ParamBufOwned {
        ParamBufOwned {
            params: self.params.iter().cloned().collect(),
        }
    }

    pub fn byte_len(&self) -> usize {
        self.params.iter().map(|p| p.len()).sum::<usize>() + self.params.len().saturating_sub(1)
    }

    pub fn numeric(&self) -> NumericParamBuf<'a> {
        NumericParamBuf {
            params: self.params,
        }
    }
}

/// A view into a [`Param`] that contains only numeric parameters.
#[derive(Debug, Copy, Clone, Default)]
pub struct NumericParam<'a> {
    pub(crate) param: &'a [u8],
}

impl<'a> NumericParam<'a> {
    /// Try to parse the parameter as a single numeric value.
    pub fn sole(&self) -> Option<u16> {
        if !self.param.is_empty() && !self.param.contains(&b':') {
            if let Ok(s) = std::str::from_utf8(self.param) {
                s.parse::<u16>().ok()
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Try to parse the first parameter as a single numeric value.
    pub fn first(&self) -> Option<u16> {
        self.into_iter().next().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.param.is_empty()
    }

    pub fn len(&self) -> usize {
        self.param.iter().filter(|p| **p == b':').count() + 1
    }

    /// Try to write the parameters to the given buffer, returning the written
    /// slice if successful. If any parameters are not a single-numeric value,
    /// writes a zero in that position.
    ///
    /// If the slice is not long enough, returns an error with the required
    /// length.
    pub fn try_write<'b>(&self, buf: &'b mut [u16]) -> Result<&'b [u16], usize> {
        let len = self.len();
        if buf.len() < len {
            return Err(buf.len());
        }
        for (i, param) in self.into_iter().enumerate() {
            buf[i] = param.unwrap_or(0);
        }
        Ok(&buf[..len])
    }
}

impl<'a> IntoIterator for NumericParam<'a> {
    type Item = Option<u16>;
    type IntoIter = Map<std::slice::Split<'a, u8, fn(&u8) -> bool>, fn(&'a [u8]) -> Option<u16>>;
    fn into_iter(self) -> Self::IntoIter {
        let fn1: fn(&u8) -> bool = |c: &u8| *c == b':';
        self.param.split(fn1).map(|p| {
            if p.is_empty() {
                None
            } else {
                std::str::from_utf8(p)
                    .ok()
                    .and_then(|s| s.parse::<u16>().ok())
            }
        })
    }
}

/// A view into a [`ParamBuf`] that contains only numeric parameters.
///
/// Each parameter may contain zero or more numeric values, separated by colons.
/// Empty parameters are interpreted as `None`.
#[derive(Debug, Copy, Clone)]
pub struct NumericParamBuf<'a> {
    pub(crate) params: &'a Params,
}

impl<'a> IntoIterator for NumericParamBuf<'a> {
    type Item = NumericParam<'a>;
    type IntoIter = Map<std::slice::Iter<'a, Param>, fn(&'a Param) -> NumericParam<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| NumericParam {
            param: p.as_slice(),
        })
    }
}

impl<'a> NumericParamBuf<'a> {
    pub const fn empty() -> Self {
        NumericParamBuf {
            params: &EMPTY_PARAMS,
        }
    }

    /// Try to write the parameters to the given buffer, returning the written
    /// slice if successful. If any parameters are not a single-numeric value,
    /// writes a zero in that position.
    ///
    /// If the slice is not long enough, returns an error with the required
    /// length.
    pub fn try_write<'b>(&self, buf: &'b mut [u16]) -> Result<&'b [u16], usize> {
        let len = self.params.len();
        if buf.len() < len {
            return Err(buf.len());
        }
        for (i, param) in self.into_iter().enumerate() {
            buf[i] = param.sole().unwrap_or(0);
        }
        Ok(&buf[..len])
    }

    pub fn get(&self, index: usize) -> Option<NumericParam<'a>> {
        self.params.get(index).map(|p| NumericParam {
            param: p.as_slice(),
        })
    }

    pub fn first(&self) -> Option<NumericParam<'a>> {
        self.into_iter().next()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn len(&self) -> usize {
        self.params.len()
    }
}

/// A union of all possible events that can be emitted by the parser, with
/// borrowed data.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum VTEvent<'a> {
    // Plain printable text from GROUND (coalesced)
    Raw(&'a [u8]),

    // C0 control (EXECUTE)
    C0(u8),

    // ESC final (with intermediates)
    Esc(Esc),

    // Invalid escape sequence
    EscInvalid(EscInvalid),

    // SS2
    Ss2(SS2),

    // SS3
    Ss3(SS3),

    // CSI short escape
    Csi(CSI<'a>),

    // DCS stream
    DcsStart(DCS<'a>),
    DcsData(&'a [u8]),
    DcsEnd(&'a [u8]),
    DcsCancel,

    // OSC stream
    OscStart,
    OscData(&'a [u8]),
    OscEnd {
        data: &'a [u8],
        /// Whether the BEL was used to end the OSC stream.
        used_bel: bool,
    },
    OscCancel,
}

impl<'a> std::fmt::Debug for VTEvent<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use VTEvent::*;
        match self {
            Raw(s) => {
                write!(f, "Raw('")?;
                fmt_utf8_bytes_with_ascii_control(f, s)?;
                write!(f, "')")?;
                Ok(())
            }
            EscInvalid(esc_invalid) => esc_invalid.fmt(f),
            C0(b) => write!(f, "C0({b:02x})"),
            Esc(esc) => esc.fmt(f),
            Ss2(ss2) => ss2.fmt(f),
            Ss3(ss3) => ss3.fmt(f),
            Csi(csi) => csi.fmt(f),
            DcsStart(dcs_start) => dcs_start.fmt(f),
            DcsData(s) | DcsEnd(s) => {
                if matches!(self, DcsEnd(..)) {
                    write!(f, "DcsEnd('")?;
                } else {
                    write!(f, "DcsData('")?;
                }
                fmt_utf8_bytes_with_ascii_control(f, s)?;
                write!(f, "')")?;
                Ok(())
            }
            DcsCancel => write!(f, "DcsCancel"),
            OscStart => write!(f, "OscStart"),
            OscData(s) | OscEnd { data: s, .. } => {
                if matches!(self, OscEnd { .. }) {
                    write!(f, "OscEnd('")?;
                } else {
                    write!(f, "OscData('")?;
                }
                fmt_utf8_bytes_with_ascii_control(f, s)?;
                write!(f, "')")?;
                Ok(())
            }
            OscCancel => write!(f, "OscCancel"),
        }
    }
}

impl<'a> VTEvent<'a> {
    pub fn csi(&self) -> Option<CSI<'a>> {
        match self {
            VTEvent::Csi(csi) => Some(CSI {
                private: csi.private,
                params: csi.params,
                intermediates: csi.intermediates,
                final_byte: csi.final_byte,
            }),
            _ => None,
        }
    }

    pub fn byte_len(&self) -> usize {
        use VTEvent::*;

        match self {
            Raw(s) => s.len(),
            C0(_) => 1,
            Esc(esc) => esc.intermediates.len() + 2 + esc.private.is_some() as usize,
            EscInvalid(esc_invalid) => {
                use self::EscInvalid::*;
                match esc_invalid {
                    One(..) => 2,
                    Two(..) => 3,
                    Three(..) => 4,
                    Four(..) => 5,
                }
            }
            Ss2(_) => 3,
            Ss3(_) => 3,
            Csi(csi) => {
                csi.private.is_some() as usize
                    + csi.params.byte_len()
                    + csi.intermediates.byte_len()
                    + 3
            }
            DcsStart(dcs_start) => {
                dcs_start.private.is_some() as usize
                    + dcs_start.params.byte_len()
                    + dcs_start.intermediates.byte_len()
                    + 3
            }
            DcsData(s) => s.len(),
            DcsEnd(s) => s.len() + 2,
            DcsCancel => 1,
            OscStart => 2,
            OscData(s) => s.len(),
            OscEnd { data, used_bel } => {
                if *used_bel {
                    data.len() + 1
                } else {
                    data.len() + 2
                }
            }
            OscCancel => 1,
        }
    }

    /// Encode the event into the provided buffer, returning the number of bytes
    /// required for the escape sequence in either `Ok(n)` or `Err(n)`.
    ///
    /// Note that some events may have multiple possible encodings, so this method
    /// may decide to choose whichever is more efficient.
    pub fn encode(&self, mut buf: &mut [u8]) -> Result<usize, usize> {
        use crate::{BEL, CAN, CSI, DCS, ESC, OSC, SS2, SS3, ST_FINAL};
        use VTEvent::*;

        let len = self.byte_len();

        if len > buf.len() {
            return Err(len);
        }

        match self {
            Raw(s) | OscData(s) | DcsData(s) => {
                buf[..s.len()].copy_from_slice(s);
            }
            EscInvalid(esc_invalid) => {
                use self::EscInvalid::*;
                buf[0] = ESC;
                match esc_invalid {
                    One(b) => buf[1] = *b,
                    Two(b1, b2) => {
                        buf[1] = *b1;
                        buf[2] = *b2;
                    }
                    Three(b1, b2, b3) => {
                        buf[1] = *b1;
                        buf[2] = *b2;
                        buf[3] = *b3;
                    }
                    Four(b1, b2, b3, b4) => {
                        buf[1] = *b1;
                        buf[2] = *b2;
                        buf[3] = *b3;
                        buf[4] = *b4;
                    }
                }
            }
            OscCancel | DcsCancel => {
                buf[0] = CAN;
            }
            C0(b) => {
                buf[0] = *b;
            }
            Ss2(ss2) => {
                buf[0] = ESC;
                buf[1] = SS2;
                buf[2] = ss2.char;
            }
            Ss3(ss3) => {
                buf[0] = ESC;
                buf[1] = SS3;
                buf[2] = ss3.char;
            }
            Esc(esc) => {
                buf[0] = ESC;
                if let Some(p) = esc.private {
                    buf[1] = p;
                    buf = &mut buf[1..];
                }
                buf[1..esc.intermediates.len() + 1]
                    .copy_from_slice(&esc.intermediates.data[..esc.intermediates.len()]);
                buf[esc.intermediates.len() + 1] = esc.final_byte;
            }
            Csi(csi) => {
                buf[0] = ESC;
                buf[1] = CSI;
                buf = &mut buf[2..];
                if let Some(p) = csi.private {
                    buf[0] = p;
                    buf = &mut buf[1..];
                }
                let mut params = csi.params.into_iter();
                if let Some(param) = params.next() {
                    buf[..param.len()].copy_from_slice(param);
                    buf = &mut buf[param.len()..];
                    for param in params {
                        buf[0] = b';';
                        buf = &mut buf[1..];
                        buf[..param.len()].copy_from_slice(param);
                        buf = &mut buf[param.len()..];
                    }
                }
                buf[..csi.intermediates.len()]
                    .copy_from_slice(&csi.intermediates.data[..csi.intermediates.len()]);
                buf[csi.intermediates.len()] = csi.final_byte;
            }
            DcsStart(dcs_start) => {
                buf[0] = ESC;
                buf[1] = DCS;
                buf = &mut buf[2..];
                if let Some(p) = dcs_start.private {
                    buf[0] = p;
                    buf = &mut buf[1..];
                }
                let mut params = dcs_start.params.into_iter();
                if let Some(param) = params.next() {
                    buf[..param.len()].copy_from_slice(param);
                    buf = &mut buf[param.len()..];
                    for param in params {
                        buf[0] = b';';
                        buf = &mut buf[1..];
                        buf[..param.len()].copy_from_slice(param);
                        buf = &mut buf[param.len()..];
                    }
                }
                buf[..dcs_start.intermediates.len()].copy_from_slice(
                    &dcs_start.intermediates.data[..dcs_start.intermediates.len()],
                );
                buf[dcs_start.intermediates.len()] = dcs_start.final_byte;
            }
            DcsEnd(data) => {
                buf[..data.len()].copy_from_slice(data);
                buf = &mut buf[data.len()..];
                buf[0] = ESC;
                buf[1] = ST_FINAL;
            }
            OscStart => {
                buf[0] = ESC;
                buf[1] = OSC;
            }
            OscEnd { data, used_bel } => {
                buf[..data.len()].copy_from_slice(data);
                buf = &mut buf[data.len()..];
                if *used_bel {
                    buf[0] = BEL;
                } else {
                    buf[0] = ESC;
                    buf[1] = ST_FINAL
                }
            }
        }

        Ok(len)
    }

    pub fn to_owned(&self) -> VTOwnedEvent {
        use VTEvent::*;
        match self {
            Raw(s) => VTOwnedEvent::Raw(s.to_vec()),
            C0(b) => VTOwnedEvent::C0(*b),
            Esc(esc) => VTOwnedEvent::Esc(*esc),
            EscInvalid(esc_invalid) => VTOwnedEvent::EscInvalid(*esc_invalid),
            Ss2(ss2) => VTOwnedEvent::Ss2(*ss2),
            Ss3(ss3) => VTOwnedEvent::Ss3(*ss3),
            Csi(csi) => VTOwnedEvent::Csi(CSIOwned {
                private: csi.private,
                params: csi.params.to_owned(),
                intermediates: csi.intermediates,
                final_byte: csi.final_byte,
            }),
            DcsStart(dcs_start) => VTOwnedEvent::DcsStart(DCSOwned {
                private: dcs_start.private,
                params: dcs_start.params.to_owned(),
                intermediates: dcs_start.intermediates,
                final_byte: dcs_start.final_byte,
            }),
            DcsData(s) => VTOwnedEvent::DcsData(s.to_vec()),
            DcsEnd(s) => VTOwnedEvent::DcsEnd(s.to_vec()),
            DcsCancel => VTOwnedEvent::DcsCancel,
            OscStart => VTOwnedEvent::OscStart,
            OscData(s) => VTOwnedEvent::OscData(s.to_vec()),
            OscEnd { data, used_bel } => VTOwnedEvent::OscEnd {
                data: data.to_vec(),
                used_bel: *used_bel,
            },
            OscCancel => VTOwnedEvent::OscCancel,
        }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ParamBufOwned {
    pub(crate) params: Params,
}

impl IntoIterator for ParamBufOwned {
    type Item = Param;
    type IntoIter = <Params as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.params.into_iter()
    }
}

impl<'b> IntoIterator for &'b ParamBufOwned {
    type Item = &'b [u8];
    type IntoIter = Map<std::slice::Iter<'b, Param>, fn(&Param) -> &[u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p.as_slice())
    }
}

impl ParamBufOwned {
    /// Create an empty `ParamBufOwned`.
    pub const fn empty() -> Self {
        Self {
            params: SmallVec::new_const(),
        }
    }

    /// Create a `ParamBufOwned` from a slice of slices.
    pub fn new(params: &[&[u8]]) -> Self {
        Self {
            params: params.iter().map(|p| Param::from(*p)).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&[u8]> {
        self.params.get(index).map(|p| p.as_slice())
    }

    pub fn try_parse<T: std::str::FromStr>(&self, index: usize) -> Option<T> {
        self.params.get(index).and_then(|p| {
            std::str::from_utf8(p.as_slice())
                .ok()
                .and_then(|s| s.parse::<T>().ok())
        })
    }

    pub fn borrow(&self) -> ParamBuf<'_> {
        ParamBuf {
            params: &self.params,
        }
    }

    pub fn numeric(&self) -> NumericParamBuf<'_> {
        NumericParamBuf {
            params: &self.params,
        }
    }
}

/// A union of all possible events that can be emitted by the parser, with owned
/// data.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq)]
pub enum VTOwnedEvent {
    Raw(Vec<u8>),
    C0(u8),
    Esc(Esc),
    EscInvalid(EscInvalid),
    Ss2(SS2),
    Ss3(SS3),
    Csi(CSIOwned),
    DcsStart(DCSOwned),
    DcsData(Vec<u8>),
    DcsEnd(Vec<u8>),
    DcsCancel,
    OscStart,
    OscData(Vec<u8>),
    OscEnd { data: Vec<u8>, used_bel: bool },
    OscCancel,
}

impl std::fmt::Debug for VTOwnedEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.borrow().fmt(f)
    }
}

impl VTOwnedEvent {
    pub fn borrow(&self) -> VTEvent<'_> {
        match self {
            VTOwnedEvent::Raw(s) => VTEvent::Raw(s),
            VTOwnedEvent::C0(b) => VTEvent::C0(*b),
            VTOwnedEvent::Esc(esc) => VTEvent::Esc(*esc),
            VTOwnedEvent::EscInvalid(esc_invalid) => VTEvent::EscInvalid(*esc_invalid),
            VTOwnedEvent::Ss2(ss2) => VTEvent::Ss2(SS2 { char: ss2.char }),
            VTOwnedEvent::Ss3(ss3) => VTEvent::Ss3(SS3 { char: ss3.char }),
            VTOwnedEvent::Csi(csi) => VTEvent::Csi(CSI {
                private: csi.private,
                params: csi.params.borrow(),
                intermediates: csi.intermediates,
                final_byte: csi.final_byte,
            }),
            VTOwnedEvent::DcsStart(dcs_start) => VTEvent::DcsStart(DCS {
                private: dcs_start.private,
                params: dcs_start.params.borrow(),
                intermediates: dcs_start.intermediates,
                final_byte: dcs_start.final_byte,
            }),
            VTOwnedEvent::DcsData(s) => VTEvent::DcsData(s),
            VTOwnedEvent::DcsEnd(s) => VTEvent::DcsEnd(s),
            VTOwnedEvent::DcsCancel => VTEvent::DcsCancel,
            VTOwnedEvent::OscStart => VTEvent::OscStart,
            VTOwnedEvent::OscData(s) => VTEvent::OscData(s),
            VTOwnedEvent::OscEnd { data, used_bel } => VTEvent::OscEnd {
                data,
                used_bel: *used_bel,
            },
            VTOwnedEvent::OscCancel => VTEvent::OscCancel,
        }
    }
}

/// An invalid escape sequence.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum EscInvalid {
    One(u8),
    Two(u8, u8),
    Three(u8, u8, u8),
    Four(u8, u8, u8, u8),
}

impl std::fmt::Debug for EscInvalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EscInvalid::One(b) => write!(f, "EscInvalid(1B {b:02X})")?,
            EscInvalid::Two(b1, b2) => write!(f, "EscInvalid(1B {b1:02X} {b2:02X})")?,
            EscInvalid::Three(b1, b2, b3) => {
                write!(f, "EscInvalid(1B {b1:02X} {b2:02X} {b3:02X})")?
            }
            EscInvalid::Four(b1, b2, b3, b4) => {
                write!(f, "EscInvalid(1B {b1:02X} {b2:02X} {b3:02X} {b4:02X})")?
            }
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Esc {
    pub intermediates: VTIntermediate,
    pub private: Option<u8>,
    pub final_byte: u8,
}

impl std::fmt::Debug for Esc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Esc(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}, ", p as char)?;
        }
        write!(f, "{:?}, ", self.intermediates)?;
        if let Ok(c) = AsciiControl::try_from(self.final_byte as char) {
            write!(f, "{c})")?;
        } else {
            write!(f, "{})", self.final_byte as char)?;
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SS2 {
    pub char: u8,
}

impl std::fmt::Debug for SS2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ss2(")?;
        if let Ok(c) = AsciiControl::try_from(self.char as char) {
            write!(f, "{c})")?;
        } else {
            write!(f, "{:?})", self.char as char)?;
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SS3 {
    pub char: u8,
}

impl std::fmt::Debug for SS3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ss3(")?;
        if let Ok(c) = AsciiControl::try_from(self.char as char) {
            write!(f, "{c})")?;
        } else {
            write!(f, "{:?})", self.char as char)?;
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CSI<'a> {
    pub private: Option<u8>,
    pub params: ParamBuf<'a>,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl<'a> std::fmt::Debug for CSI<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Csi(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}, ", p as char)?;
        }
        for param in &self.params {
            write!(f, "'")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "', ")?;
        }
        write!(f, "{:?}, ", self.intermediates)?;
        write!(f, "{:?})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DCS<'a> {
    pub private: Option<u8>,
    pub params: ParamBuf<'a>,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl<'a> std::fmt::Debug for DCS<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DcsStart(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}, ", p as char)?;
        }
        for param in &self.params {
            write!(f, "'")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "', ")?;
        }
        write!(f, "{:?}, ", self.intermediates)?;
        write!(f, "{})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq)]
pub struct CSIOwned {
    pub private: Option<u8>,
    pub params: ParamBufOwned,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl std::fmt::Debug for CSIOwned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Csi(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}", p as char)?;
        }
        for param in &self.params {
            write!(f, ", '")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "'")?;
        }
        write!(f, ", {:?}", self.intermediates)?;
        write!(f, ", {:?})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq)]
pub struct DCSOwned {
    pub private: Option<u8>,
    pub params: ParamBufOwned,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl std::fmt::Debug for DCSOwned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DcsStart(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}", p as char)?;
        }
        for param in &self.params {
            write!(f, ", '")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "'")?;
        }
        write!(f, ", {:?}", self.intermediates)?;
        write!(f, ", {})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_numeric_param_buf() {
        let param_buf = ParamBufOwned::new(&[b"1:2:3", b"4", b":"]);
        let numeric_param_buf = param_buf.numeric();
        assert_eq!(
            numeric_param_buf.into_iter().flatten().collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3), Some(4), None, None]
        );

        assert_eq!(numeric_param_buf.first().unwrap().sole(), None);
        assert_eq!(numeric_param_buf.first().unwrap().first(), Some(1));
        assert_eq!(numeric_param_buf.get(1).unwrap().sole(), Some(4));
        assert_eq!(numeric_param_buf.get(1).unwrap().first(), Some(4));
        assert_eq!(numeric_param_buf.get(2).unwrap().sole(), None);
        assert_eq!(numeric_param_buf.get(2).unwrap().first(), None);

        assert_eq!(
            numeric_param_buf
                .try_write(&mut [0, 0, 0, 0, 0, 0])
                .unwrap(),
            &[0, 4, 0]
        );
        assert_eq!(
            numeric_param_buf
                .get(0)
                .unwrap()
                .try_write(&mut [0, 0, 0, 0, 0, 0])
                .unwrap(),
            &[1, 2, 3]
        );
    }
}
