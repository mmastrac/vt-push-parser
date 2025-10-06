//! Escape sequence signature matching.

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
    pub const fn with_private(self, private: u8) -> Self {
        Self {
            private: Some(private),
            ..self
        }
    }

    pub const fn with_intermediate(self, intermediate: u8) -> Self {
        Self {
            intermediates: VTIntermediate::one(intermediate),
            ..self
        }
    }

    pub const fn with_params_exact(self, param_count: u8) -> Self {
        Self {
            param_count: param_count..param_count + 1,
            ..self
        }
    }

    pub const fn with_params_count(self, param_count: Range<u8>) -> Self {
        Self {
            param_count,
            ..self
        }
    }

    pub const fn csi(final_byte: u8) -> Self {
        Self {
            prefix: CSI,
            final_byte,
            param_count: 0..1,
            intermediates: VTIntermediate::empty(),
            private: None,
        }
    }

    pub const fn ss3(final_byte: u8) -> Self {
        Self {
            prefix: SS3,
            private: None,
            intermediates: VTIntermediate::empty(),
            final_byte,
            param_count: 0..1,
        }
    }

    pub const fn dcs(final_byte: u8) -> Self {
        Self {
            prefix: DCS,
            private: None,
            intermediates: VTIntermediate::empty(),
            final_byte,
            param_count: 0..1,
        }
    }

    pub const fn osc(final_byte: u8) -> Self {
        Self {
            prefix: OSC,
            private: None,
            intermediates: VTIntermediate::empty(),
            final_byte,
            param_count: 0..1,
        }
    }

    pub fn matches(&self, entry: &VTEvent) -> bool {
        // TODO: const
        match entry {
            VTEvent::Esc(esc) => {
                self.final_byte == esc.final_byte && self.intermediates.const_eq(&esc.intermediates)
            }
            VTEvent::Csi(csi) => {
                self.prefix == CSI
                    && self.final_byte == csi.final_byte
                    && self.intermediates.const_eq(&csi.intermediates)
                    && self.const_private_eq(&csi.private)
                    && self.const_contains(csi.params.len())
            }
            VTEvent::DcsStart(dcs_start) => {
                self.prefix == DCS
                    && self.final_byte == dcs_start.final_byte
                    && self.intermediates.const_eq(&dcs_start.intermediates)
                    && self.private == dcs_start.private
                    && self.const_contains(dcs_start.params.len())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VTPushParser;

    const CURSOR_POSITION_REPORT: VTEscapeSignature =
        VTEscapeSignature::csi(b'n').with_params_exact(2);

    #[test]
    fn test_matches() {
        let input = b"\x1b[1;2n";
        let mut found = false;
        VTPushParser::decode_buffer(input, |event| {
            assert!(!found);
            found = true;
            assert!(CURSOR_POSITION_REPORT.matches(&event));
        });
        assert!(found);
    }
}
