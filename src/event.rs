use std::iter::Map;

use crate::AsciiControl;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct VTIntermediate {
    data: [u8; 2],
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

    #[must_use]
    pub fn push(&mut self, c: u8) -> bool {
        if c < 0x20 || c > 0x2F {
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

#[derive(PartialEq, Eq)]
#[repr(transparent)]
pub struct ParamBuf<'a> {
    pub(crate) params: &'a Vec<Vec<u8>>,
}

impl<'a> IntoIterator for ParamBuf<'a> {
    type Item = &'a [u8];
    type IntoIter = Map<std::slice::Iter<'a, Vec<u8>>, fn(&Vec<u8>) -> &[u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p.as_slice())
    }
}

impl<'b, 'a> IntoIterator for &'b ParamBuf<'a> {
    type Item = &'a [u8];
    type IntoIter = Map<std::slice::Iter<'a, Vec<u8>>, fn(&Vec<u8>) -> &[u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p.as_slice())
    }
}

impl<'a> ParamBuf<'a> {
    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn to_owned(&self) -> ParamBufOwned {
        ParamBufOwned {
            params: self.params.iter().map(|p| p.to_vec()).collect(),
        }
    }
}

pub enum VTEvent<'a> {
    // Plain printable text from GROUND (coalesced)
    Raw(&'a [u8]),

    // C0 control (EXECUTE)
    C0(u8),

    // ESC final (with intermediates)
    Esc {
        intermediates: VTIntermediate,
        final_byte: u8,
    },

    // CSI short escape
    Csi {
        private: Option<u8>,
        params: ParamBuf<'a>,
        intermediates: VTIntermediate,
        final_byte: u8,
    },

    // SS3 (ESC O …)
    Ss3 {
        intermediates: VTIntermediate,
        final_byte: u8,
    },

    // DCS stream
    DcsStart {
        priv_prefix: Option<u8>,
        params: ParamBuf<'a>,
        intermediates: VTIntermediate,
        final_byte: u8,
    },
    DcsData(&'a [u8]),
    DcsEnd,
    DcsCancel,

    // OSC stream
    OscStart,
    OscData(&'a [u8]),
    OscEnd {
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
                for chunk in s.utf8_chunks() {
                    for c in chunk.valid().chars() {
                        if let Ok(c) = AsciiControl::try_from(c) {
                            write!(f, "{}", c)?;
                        } else {
                            write!(f, "{}", c)?;
                        }
                    }
                    if !chunk.invalid().is_empty() {
                        write!(f, "<{}>", hex::encode(chunk.invalid()))?;
                    }
                }
                write!(f, "')")?;
                Ok(())
            }
            C0(b) => write!(f, "C0({:02x})", b),
            Esc {
                intermediates,
                final_byte,
            } => {
                write!(f, "Esc({:?}", intermediates)?;
                write!(f, ", {})", *final_byte as char)?;
                Ok(())
            }
            Csi {
                private,
                params,
                intermediates,
                final_byte,
            } => {
                write!(f, "Csi(")?;
                if let Some(p) = private {
                    write!(f, "{:?}", *p as char)?;
                }
                for param in params {
                    write!(f, ", '")?;
                    for chunk in param.utf8_chunks() {
                        write!(f, "{}", chunk.valid())?;
                        if !chunk.invalid().is_empty() {
                            write!(f, "<{}>", hex::encode(chunk.invalid()))?;
                        }
                    }
                    write!(f, "'")?;
                }
                write!(f, ", {:?}", intermediates)?;
                write!(f, ", {:?})", *final_byte as char)?;
                Ok(())
            }
            Ss3 {
                intermediates,
                final_byte,
            } => {
                write!(f, "Ss3(")?;
                write!(f, "{:?}", intermediates)?;
                write!(f, ", {})", *final_byte as char)?;
                Ok(())
            }
            DcsStart {
                priv_prefix,
                params,
                intermediates,
                final_byte,
            } => {
                write!(f, "DcsStart(")?;
                if let Some(p) = priv_prefix {
                    write!(f, "{:?}", *p as char)?;
                }
                for param in params {
                    write!(f, ", '")?;
                    for chunk in param.utf8_chunks() {
                        write!(f, "{}", chunk.valid())?;
                        if !chunk.invalid().is_empty() {
                            write!(f, "<{}>", hex::encode(chunk.invalid()))?;
                        }
                    }
                    write!(f, "'")?;
                }
                write!(f, ", {:?}", intermediates)?;
                write!(f, ", {})", *final_byte as char)?;
                Ok(())
            }
            DcsData(s) => {
                write!(f, "DcsData('")?;
                for chunk in s.utf8_chunks() {
                    for c in chunk.valid().chars() {
                        if let Ok(c) = AsciiControl::try_from(c) {
                            write!(f, "{}", c)?;
                        } else {
                            write!(f, "{}", c)?;
                        }
                    }
                    if !chunk.invalid().is_empty() {
                        write!(f, "<{}>", hex::encode(chunk.invalid()))?;
                    }
                }
                write!(f, "')")?;
                Ok(())
            }
            DcsEnd => write!(f, "DcsEnd"),
            DcsCancel => write!(f, "DcsCancel"),
            OscStart => write!(f, "OscStart"),
            OscData(s) => {
                write!(f, "OscData('")?;
                for chunk in s.utf8_chunks() {
                    for c in chunk.valid().chars() {
                        if let Ok(c) = AsciiControl::try_from(c) {
                            write!(f, "{}", c)?;
                        } else {
                            write!(f, "{}", c)?;
                        }
                    }
                    if !chunk.invalid().is_empty() {
                        write!(f, "<{}>", hex::encode(chunk.invalid()))?;
                    }
                }
                write!(f, "')")?;
                Ok(())
            }
            OscEnd { .. } => {
                write!(f, "OscEnd")?;
                Ok(())
            }
            OscCancel => write!(f, "OscCancel"),
        }
    }
}

impl<'a> VTEvent<'a> {
    pub fn to_owned(&self) -> VTOwnedEvent {
        use VTEvent::*;
        match self {
            Raw(s) => VTOwnedEvent::Raw(s.to_vec()),
            C0(b) => VTOwnedEvent::C0(*b),
            Esc {
                intermediates,
                final_byte,
            } => VTOwnedEvent::Esc {
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            Csi {
                private,
                params,
                intermediates,
                final_byte,
            } => VTOwnedEvent::Csi {
                private: private.clone(),
                params: params.to_owned(),
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            Ss3 {
                intermediates,
                final_byte,
            } => VTOwnedEvent::Ss3 {
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            DcsStart {
                priv_prefix,
                params,
                intermediates,
                final_byte,
            } => VTOwnedEvent::DcsStart {
                priv_prefix: priv_prefix.clone(),
                params: params.to_owned(),
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            DcsData(s) => VTOwnedEvent::DcsData(s.to_vec()),
            DcsEnd => VTOwnedEvent::DcsEnd,
            DcsCancel => VTOwnedEvent::DcsCancel,
            OscStart => VTOwnedEvent::OscStart,
            OscData(s) => VTOwnedEvent::OscData(s.to_vec()),
            OscEnd { used_bel } => VTOwnedEvent::OscEnd {
                used_bel: *used_bel,
            },
            OscCancel => VTOwnedEvent::OscCancel,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ParamBufOwned {
    pub(crate) params: Vec<Vec<u8>>,
}

impl IntoIterator for ParamBufOwned {
    type Item = Vec<u8>;
    type IntoIter = std::vec::IntoIter<Vec<u8>>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.into_iter()
    }
}

impl<'b> IntoIterator for &'b ParamBufOwned {
    type Item = &'b [u8];
    type IntoIter = Map<std::slice::Iter<'b, Vec<u8>>, fn(&Vec<u8>) -> &[u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p.as_slice())
    }
}

impl ParamBufOwned {
    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn borrow(&self) -> ParamBuf<'_> {
        ParamBuf {
            params: &self.params,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum VTOwnedEvent {
    Raw(Vec<u8>),
    C0(u8),
    Esc {
        intermediates: VTIntermediate,
        final_byte: u8,
    },
    Csi {
        private: Option<u8>,
        params: ParamBufOwned,
        intermediates: VTIntermediate,
        final_byte: u8,
    },
    Ss3 {
        intermediates: VTIntermediate,
        final_byte: u8,
    },
    DcsStart {
        priv_prefix: Option<u8>,
        params: ParamBufOwned,
        intermediates: VTIntermediate,
        final_byte: u8,
    },
    DcsData(Vec<u8>),
    DcsEnd,
    DcsCancel,
    OscStart,
    OscData(Vec<u8>),
    OscEnd {
        used_bel: bool,
    },
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
            VTOwnedEvent::Esc {
                intermediates,
                final_byte,
            } => VTEvent::Esc {
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            VTOwnedEvent::Csi {
                private,
                params,
                intermediates,
                final_byte,
            } => VTEvent::Csi {
                private: private.clone(),
                params: params.borrow(),
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            VTOwnedEvent::Ss3 {
                intermediates,
                final_byte,
            } => VTEvent::Ss3 {
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            VTOwnedEvent::DcsStart {
                priv_prefix,
                params,
                intermediates,
                final_byte,
            } => VTEvent::DcsStart {
                priv_prefix: priv_prefix.clone(),
                params: params.borrow(),
                intermediates: intermediates.clone(),
                final_byte: *final_byte,
            },
            VTOwnedEvent::DcsData(s) => VTEvent::DcsData(s),
            VTOwnedEvent::DcsEnd => VTEvent::DcsEnd,
            VTOwnedEvent::DcsCancel => VTEvent::DcsCancel,
            VTOwnedEvent::OscStart => VTEvent::OscStart,
            VTOwnedEvent::OscData(s) => VTEvent::OscData(s),
            VTOwnedEvent::OscEnd { used_bel } => VTEvent::OscEnd {
                used_bel: *used_bel,
            },
            VTOwnedEvent::OscCancel => VTEvent::OscCancel,
        }
    }
}
