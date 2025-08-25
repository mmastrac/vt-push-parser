use std::ops::Range;

use crate::event::{VTEvent, VTIntermediate};

const CSI: u8 = b'[';
const SS3: u8 = b'O';
const DCS: u8 = b'P';
const OSC: u8 = b']';

/// A signature for an escape sequence.
pub struct VTEscapeSignature {
    pub prefix: u8,
    pub private: Option<u8>,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
    pub param_count: Range<u8>,
}

impl VTEscapeSignature {
    pub const fn csi(
        private: Option<u8>,
        param_count: Range<u8>,
        intermediates: VTIntermediate,
        final_byte: u8,
    ) -> Self {
        Self {
            prefix: CSI,
            private,
            intermediates,
            final_byte,
            param_count,
        }
    }

    pub const fn ss3(intermediates: VTIntermediate, final_byte: u8) -> Self {
        Self {
            prefix: SS3,
            private: None,
            intermediates,
            final_byte,
            param_count: u8::MIN..u8::MAX,
        }
    }

    pub const fn dcs(
        priv_prefix: Option<u8>,
        param_count: Range<u8>,
        intermediates: VTIntermediate,
        final_byte: u8,
    ) -> Self {
        Self {
            prefix: DCS,
            private: priv_prefix,
            intermediates,
            final_byte,
            param_count,
        }
    }

    pub const fn osc(intermediates: VTIntermediate, final_byte: u8) -> Self {
        Self {
            prefix: OSC,
            private: None,
            intermediates,
            final_byte,
            param_count: u8::MIN..u8::MAX,
        }
    }

    pub fn matches(&self, entry: &VTEvent) -> bool {
        // TODO: const
        match entry {
            VTEvent::Esc {
                intermediates,
                final_byte,
            } => self.final_byte == *final_byte && self.intermediates.const_eq(intermediates),
            VTEvent::Csi {
                private,
                params,
                intermediates,
                final_byte,
            } => {
                self.prefix == CSI
                    && self.final_byte == *final_byte
                    && self.intermediates.const_eq(intermediates)
                    && self.const_private_eq(private)
                    && self.const_contains(params.len())
            }
            VTEvent::Ss3 {
                intermediates,
                final_byte,
            } => {
                self.prefix == SS3
                    && self.final_byte == *final_byte
                    && self.intermediates.const_eq(intermediates)
            }
            VTEvent::DcsStart {
                priv_prefix,
                params,
                intermediates,
                final_byte,
            } => {
                self.prefix == DCS
                    && self.final_byte == *final_byte
                    && self.intermediates.const_eq(intermediates)
                    && self.private == *priv_prefix
                    && self.const_contains(params.len())
            }
            _ => false,
        }
    }

    const fn const_private_eq(&self, other: &Option<u8>) -> bool {
        match (self.private, other) {
            (Some(a), Some(b)) => a == *b,
            (None, None) => true,
            _ => false,
        }
    }

    fn const_contains(&self, len: usize) -> bool {
        // TODO: const
        self.param_count.contains(&(len as u8))
    }
}
