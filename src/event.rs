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
        params: smallvec::SmallVec<[&'a [u8]; 4]>,
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
        params: smallvec::SmallVec<[&'a [u8]; 4]>,
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
        match self {
            VTEvent::Raw(s) => {
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
            VTEvent::C0(b) => write!(f, "C0({:02x})", b),
            VTEvent::Esc {
                intermediates,
                final_byte,
            } => {
                write!(f, "Esc({:?}", intermediates)?;
                write!(f, ", {})", *final_byte as char)?;
                Ok(())
            }
            VTEvent::Csi {
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
            VTEvent::Ss3 {
                intermediates,
                final_byte,
            } => {
                write!(f, "Ss3(")?;
                write!(f, "{:?}", intermediates)?;
                write!(f, ", {})", *final_byte as char)?;
                Ok(())
            }
            VTEvent::DcsStart {
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
            VTEvent::DcsData(s) => {
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
            VTEvent::DcsEnd => write!(f, "DcsEnd"),
            VTEvent::DcsCancel => write!(f, "DcsCancel"),
            VTEvent::OscStart => write!(f, "OscStart"),
            VTEvent::OscData(s) => {
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
            VTEvent::OscEnd { .. } => {
                write!(f, "OscEnd")?;
                Ok(())
            }
            VTEvent::OscCancel => write!(f, "OscCancel"),
        }
    }
}
