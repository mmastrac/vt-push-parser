pub mod ascii;
pub mod event;
pub mod signature;

use smallvec::SmallVec;

use ascii::AsciiControl;

const ESC: u8 = AsciiControl::Esc as _;
const BEL: u8 = AsciiControl::Bel as _;
const DEL: u8 = AsciiControl::Del as _;
const CAN: u8 = AsciiControl::Can as _;
const SUB: u8 = AsciiControl::Sub as _;
const CSI: u8 = b'[';
const OSC: u8 = b']';
const SS3: u8 = b'O';
const DCS: u8 = b'P';
const ST_FINAL: u8 = b'\\';

// Re-export the main types for backward compatibility
pub use event::{VTEvent, VTIntermediate};
pub use signature::VTEscapeSignature;

use crate::event::ParamBuf;

/// The action to take with the most recently accumulated byte.
pub enum VTAction<'a> {
    /// The parser will accumulate the byte and continue processing. If
    /// currently buffered, emit the buffered bytes.
    None,
    /// The parser emitted an event. If currently buffered, emit the buffered
    /// bytes.
    Event(VTEvent<'a>),
    /// Start or continue buffering bytes. Include the current byte in the
    /// buffer.
    Buffer(VTEmit),
    /// Hold this byte until the next byte is received. If another byte is
    /// already held, emit the previous byte.
    Hold(VTEmit),
    /// Cancel the current buffer.
    Cancel(VTEmit),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VTEmit {
    /// Emit this byte as a ground-state character.
    Ground,
    /// Emit this byte into the current DCS stream.
    Dcs,
    /// Emit this byte into the current OSC stream.
    Osc,
}

#[inline]
fn is_c0(b: u8) -> bool {
    b <= 0x1F
}
#[inline]
fn is_printable(b: u8) -> bool {
    (0x20..=0x7E).contains(&b)
}
#[inline]
fn is_intermediate(b: u8) -> bool {
    (0x20..=0x2F).contains(&b)
}
#[inline]
fn is_final(b: u8) -> bool {
    (0x30..=0x7E).contains(&b)
}
#[inline]
fn is_digit(b: u8) -> bool {
    (b'0'..=b'9').contains(&b)
}
#[inline]
fn is_priv(b: u8) -> bool {
    matches!(b, b'<' | b'=' | b'>' | b'?')
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    EscInt,
    CsiEntry,
    CsiParam,
    CsiInt,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsInt,
    DcsIgnore,
    DcsIgnoreEsc,
    DcsPassthrough,
    DcsEsc,
    OscString,
    OscEsc,
    SosPmApcString,
    SpaEsc,
}

pub struct VTPushParser {
    st: State,

    // Header collectors for short escapes (we borrow from these in callbacks)
    ints: VTIntermediate,
    params: Vec<Vec<u8>>,
    cur_param: Vec<u8>,
    priv_prefix: Option<u8>,
    held_byte: Option<u8>,

    // Streaming buffer (DCS/OSC bodies)
    used_bel: bool,
}

impl VTPushParser {
    pub fn new() -> Self {
        Self {
            st: State::Ground,
            ints: VTIntermediate::default(),
            params: Vec::with_capacity(8),
            cur_param: Vec::with_capacity(8),
            priv_prefix: None,
            held_byte: None,
            used_bel: false,
        }
    }

    /// Decode a buffer of bytes into a series of events.
    pub fn decode_buffer<'a>(input: &'a [u8], mut cb: impl for<'b> FnMut(VTEvent<'b>)) {
        let mut parser = Self::new();
        parser.feed_with(input, &mut cb);
        parser.finish(&mut cb);
    }

    // =====================
    // Callback-driven API
    // =====================

    pub fn feed_with<'this: 'input, 'input, F: for<'any> FnMut(VTEvent<'any>)>(
        &'this mut self,
        input: &'input [u8],
        cb: &mut F,
    ) {
        let mut buffer_idx = 0;
        let mut current_emit = None;
        let mut held_byte = self.held_byte.take();
        let mut hold = held_byte.is_some();

        let mut action_handler = |mut i: usize, action: VTAction| {
            // Special case: carryover from previous. We need to emit it on its own.
            if let Some(b) = held_byte.take() {
                match action {
                    VTAction::Buffer(emit) => match emit {
                        VTEmit::Ground => cb(VTEvent::Raw(&[b])),
                        VTEmit::Dcs => cb(VTEvent::DcsData(&[b])),
                        VTEmit::Osc => cb(VTEvent::OscData(&[b])),
                    },
                    _ => {}
                }
            }

            let mut emit_buffer = |emit: VTEmit, hold: bool| {
                if hold {
                    i = i - 1;
                }

                if (buffer_idx..i).len() > 0 {
                    match emit {
                        VTEmit::Ground => cb(VTEvent::Raw(&input[buffer_idx..i])),
                        VTEmit::Dcs => cb(VTEvent::DcsData(&input[buffer_idx..i])),
                        VTEmit::Osc => cb(VTEvent::OscData(&input[buffer_idx..i])),
                    }
                }
            };

            match action {
                VTAction::None => match current_emit.take() {
                    Some(emit) => emit_buffer(emit, hold),
                    None => {}
                },
                VTAction::Event(e) => {
                    match current_emit.take() {
                        Some(emit) => emit_buffer(emit, hold),
                        None => {}
                    };
                    cb(e);
                }
                VTAction::Buffer(emit) | VTAction::Hold(emit) => {
                    hold = matches!(action, VTAction::Hold(_));
                    match current_emit {
                        None => {
                            buffer_idx = i;
                            current_emit = Some(emit);
                        }
                        Some(x) if x == emit => {}
                        Some(_) => {
                            match current_emit.take() {
                                Some(emit) => emit_buffer(emit, hold),
                                None => {}
                            }
                            buffer_idx = i;
                            current_emit = Some(emit);
                        }
                    }
                }
                VTAction::Cancel(emit) => {
                    current_emit = None;
                    match emit {
                        VTEmit::Ground => unreachable!(),
                        VTEmit::Dcs => cb(VTEvent::DcsCancel),
                        VTEmit::Osc => cb(VTEvent::OscCancel),
                    }
                }
            }
        };

        for (i, &b) in input.iter().enumerate() {
            action_handler(i, self.push_with(b));
        }

        action_handler(input.len(), VTAction::None);
    }

    pub fn push_with<'this, 'input>(&'this mut self, b: u8) -> VTAction<'this> {
        use State::*;
        match self.st {
            Ground => self.on_ground(b),
            Escape => self.on_escape(b),
            EscInt => self.on_esc_int(b),

            CsiEntry => self.on_csi_entry(b),
            CsiParam => self.on_csi_param(b),
            CsiInt => self.on_csi_int(b),
            CsiIgnore => self.on_csi_ignore(b),

            DcsEntry => self.on_dcs_entry(b),
            DcsParam => self.on_dcs_param(b),
            DcsInt => self.on_dcs_int(b),
            DcsIgnore => self.on_dcs_ignore(b),
            DcsIgnoreEsc => self.on_dcs_ignore_esc(b),
            DcsPassthrough => self.on_dcs_pass(b),
            DcsEsc => self.on_dcs_esc(b),

            OscString => self.on_osc_string(b),
            OscEsc => self.on_osc_esc(b),

            SosPmApcString => self.on_spa_string(b),
            SpaEsc => self.on_spa_esc(b),
        }
    }

    pub fn finish<F: FnMut(VTEvent)>(&mut self, cb: &mut F) {
        self.reset_collectors();
        self.st = State::Ground;

        // TODO
    }

    // =====================
    // Emit helpers (borrowed)
    // =====================

    fn clear_hdr_collectors(&mut self) {
        self.ints.clear();
        self.params.clear();
        self.cur_param.clear();
        self.priv_prefix = None;
    }

    fn reset_collectors(&mut self) {
        self.clear_hdr_collectors();
        self.used_bel = false;
    }

    fn next_param(&mut self) {
        self.params.push(std::mem::take(&mut self.cur_param));
    }

    fn finish_params_if_any(&mut self) {
        if !self.cur_param.is_empty() || !self.params.is_empty() {
            self.next_param();
        }
    }

    fn emit_csi(&mut self, final_byte: u8) -> VTAction {
        self.finish_params_if_any();

        // Build borrowed views into self.params
        let mut borrowed: SmallVec<[&[u8]; 4]> = SmallVec::new();
        borrowed.extend(self.params.iter().map(|v| v.as_slice()));

        let privp = self.priv_prefix.take();
        VTAction::Event(VTEvent::Csi {
            private: privp,
            params: ParamBuf {
                params: &self.params,
            },
            intermediates: self.ints,
            final_byte,
        })
    }

    fn dcs_start(&mut self, final_byte: u8) -> VTAction {
        self.finish_params_if_any();

        let privp = self.priv_prefix.take();
        VTAction::Event(VTEvent::DcsStart {
            priv_prefix: privp,
            params: ParamBuf {
                params: &self.params,
            },
            intermediates: self.ints,
            final_byte,
        })
    }

    // =====================
    // State handlers
    // =====================

    fn on_ground(&mut self, b: u8) -> VTAction {
        match b {
            ESC => {
                self.clear_hdr_collectors();
                self.st = State::Escape;
                VTAction::None
            }
            DEL => VTAction::Event(VTEvent::C0(DEL)),
            c if is_c0(c) => VTAction::Event(VTEvent::C0(c)),
            p if is_printable(p) => VTAction::Buffer(VTEmit::Ground),
            _ => VTAction::Buffer(VTEmit::Ground), // safe fallback
        }
    }

    fn on_escape(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = EscInt;
                VTAction::None
            }
            CSI => {
                self.st = CsiEntry;
                VTAction::None
            }
            DCS => {
                self.st = DcsEntry;
                VTAction::None
            }
            OSC => {
                self.used_bel = false;
                self.st = OscString;
                VTAction::Event(VTEvent::OscStart)
            }
            b'X' | b'^' | b'_' => {
                self.st = State::SosPmApcString;
                VTAction::None
            }
            c if is_final(c) => {
                self.st = Ground;
                VTAction::Event(VTEvent::Esc {
                    intermediates: self.ints,
                    final_byte: c,
                })
            }
            ESC => {
                // ESC ESC allowed, but we stay in the current state
                VTAction::None
            }
            _ => {
                self.st = Ground;
                VTAction::None
            }
        }
    }

    fn on_esc_int(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            c if is_intermediate(c) => {
                self.ints.push(c);
                VTAction::None
            }
            c if is_final(c) => {
                self.st = Ground;
                VTAction::Event(VTEvent::Esc {
                    intermediates: self.ints,
                    final_byte: c,
                })
            }
            _ => {
                self.st = Ground;
                VTAction::None
            }
        }
    }

    // ---- CSI
    fn on_csi_entry(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = Escape;
                VTAction::None
            }
            c if is_priv(c) => {
                self.priv_prefix = Some(c);
                self.st = CsiParam;
                VTAction::None
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
                self.st = CsiParam;
                VTAction::None
            }
            b';' => {
                self.next_param();
                self.st = CsiParam;
                VTAction::None
            }
            b':' => {
                self.cur_param.push(b':');
                self.st = CsiParam;
                VTAction::None
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = CsiInt;
                VTAction::None
            }
            c if is_final(c) => {
                self.st = Ground;
                self.emit_csi(c)
            }
            _ => {
                self.st = CsiIgnore;
                VTAction::None
            }
        }
    }

    fn on_csi_param(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = Escape;
                VTAction::None
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
                VTAction::None
            }
            b';' => {
                self.next_param();
                VTAction::None
            }
            b':' => {
                self.cur_param.push(b':');
                VTAction::None
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = CsiInt;
                VTAction::None
            }
            c if is_final(c) => {
                self.st = Ground;
                self.emit_csi(c)
            }
            _ => {
                self.st = CsiIgnore;
                VTAction::None
            }
        }
    }

    fn on_csi_int(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = Escape;
                VTAction::None
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                VTAction::None
            }
            c if is_final(c) => {
                self.st = Ground;
                self.emit_csi(c)
            }
            _ => {
                self.st = CsiIgnore;
                VTAction::None
            }
        }
    }

    fn on_csi_ignore(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = Escape;
                VTAction::None
            }
            c if is_final(c) => {
                self.st = Ground;
                VTAction::None
            }
            _ => VTAction::None,
        }
    }

    // ---- DCS
    fn on_dcs_entry(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = Escape;
                VTAction::None
            }
            c if is_priv(c) => {
                self.priv_prefix = Some(c);
                self.st = DcsParam;
                VTAction::None
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
                self.st = DcsParam;
                VTAction::None
            }
            b';' => {
                self.next_param();
                self.st = DcsParam;
                VTAction::None
            }
            b':' => {
                self.st = DcsIgnore;
                VTAction::None
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = DcsInt;
                VTAction::None
            }
            c if is_final(c) => {
                self.st = DcsPassthrough;
                self.dcs_start(c)
            }
            _ => {
                self.st = DcsIgnore;
                VTAction::None
            }
        }
    }

    fn on_dcs_param(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = Escape;
                VTAction::None
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
                VTAction::None
            }
            b';' => {
                self.next_param();
                VTAction::None
            }
            b':' => {
                self.st = DcsIgnore;
                VTAction::None
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = DcsInt;
                VTAction::None
            }
            c if is_final(c) => {
                self.st = DcsPassthrough;
                self.dcs_start(c)
            }
            _ => {
                self.st = DcsIgnore;
                VTAction::None
            }
        }
    }

    fn on_dcs_int(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = Escape;
                VTAction::None
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                VTAction::None
            }
            c if is_final(c) => {
                self.st = DcsPassthrough;
                self.dcs_start(c)
            }
            _ => {
                self.st = DcsIgnore;
                VTAction::None
            }
        }
    }

    fn on_dcs_ignore(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = DcsIgnoreEsc;
                VTAction::None
            }
            _ => VTAction::None,
        }
    }

    fn on_dcs_ignore_esc(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            ST_FINAL => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            _ => {
                self.st = DcsIgnore;
                VTAction::None
            }
        }
    }

    fn on_dcs_pass(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::Cancel(VTEmit::Dcs)
            }
            DEL => VTAction::None,
            ESC => {
                self.st = DcsEsc;
                VTAction::Hold(VTEmit::Dcs)
            }
            _ => VTAction::Buffer(VTEmit::Dcs),
        }
    }

    fn on_dcs_esc(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            ST_FINAL => {
                self.st = Ground;
                VTAction::Event(VTEvent::DcsEnd)
            }
            ESC => {
                // If we get ESC ESC, we need to yield the previous ESC as well.
                VTAction::Hold(VTEmit::Dcs)
            }
            _ => {
                // If we get ESC !ST, we need to yield the previous ESC as well.
                self.st = DcsPassthrough;
                VTAction::Buffer(VTEmit::Dcs)
            }
        }
    }

    // ---- OSC
    fn on_osc_string(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::Cancel(VTEmit::Osc)
            }
            DEL => VTAction::None,
            BEL => {
                self.used_bel = true;
                self.st = Ground;
                VTAction::Event(VTEvent::OscEnd {
                    used_bel: self.used_bel,
                })
            }
            ESC => {
                self.st = OscEsc;
                VTAction::Hold(VTEmit::Osc)
            }
            p if is_printable(p) => VTAction::Buffer(VTEmit::Osc),
            _ => VTAction::None, // ignore other C0
        }
    }

    fn on_osc_esc(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            ST_FINAL => {
                self.used_bel = false;
                self.st = Ground;
                VTAction::Event(VTEvent::OscEnd {
                    used_bel: self.used_bel,
                })
            } // ST
            ESC => VTAction::Hold(VTEmit::Osc),
            _ => {
                self.st = OscString;
                VTAction::Buffer(VTEmit::Osc)
            }
        }
    }

    // ---- SOS/PM/APC (ignored payload)
    fn on_spa_string(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                VTAction::None
            }
            DEL => VTAction::None,
            ESC => {
                self.st = SpaEsc;
                VTAction::None
            }
            _ => VTAction::None,
        }
    }

    fn on_spa_esc(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            ST_FINAL => {
                self.st = Ground;
                VTAction::None
            }
            ESC => {
                /* remain */
                VTAction::None
            }
            _ => {
                self.st = State::SosPmApcString;
                VTAction::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_edge_cases() {
        // Test empty input
        let mut result = String::new();
        VTPushParser::decode_buffer(&[], |e| result.push_str(&format!("{:?}\n", e)));
        assert_eq!(result.trim(), "");

        // Test single ESC
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1b", |e| result.push_str(&format!("{:?}\n", e)));
        assert_eq!(result.trim(), "");

        // Test incomplete CSI
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1b[", |e| result.push_str(&format!("{:?}\n", e)));
        assert_eq!(result.trim(), "");

        // Test incomplete DCS
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1bP", |e| result.push_str(&format!("{:?}\n", e)));
        assert_eq!(result.trim(), "");

        // Test incomplete OSC
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1b]", |e| result.push_str(&format!("{:?}\n", e)));
        assert_eq!(result.trim(), "OscStart");
    }

    #[test]
    fn test_streaming_behavior() {
        // Test streaming DCS data
        let mut parser = VTPushParser::new(); // Small flush size
        let mut result = String::new();
        let mut callback = |vt_input: VTEvent<'_>| {
            result.push_str(&format!("{:?}\n", vt_input));
        };

        // Feed DCS data in chunks
        parser.feed_with(b"\x1bP1;2;3 |", &mut callback);
        parser.feed_with(b"data", &mut callback);
        parser.feed_with(b" more", &mut callback);
        parser.feed_with(b"\x1b\\", &mut callback);

        assert_eq!(
            result.trim(),
            "DcsStart(, '1', '2', '3', ' ', |)\nDcsData('data')\nDcsData(' more')\nDcsEnd"
        );
    }

    #[test]
    fn test_finish_method() {
        let mut parser = VTPushParser::new();
        let mut result = String::new();
        let mut callback = |vt_input: VTEvent<'_>| {
            result.push_str(&format!("{:?}\n", vt_input));
        };

        // Start an incomplete sequence
        parser.feed_with(b"\x1b[1;2;3", &mut callback);

        // Finish should flush any pending raw data
        parser.finish(&mut callback);

        assert_eq!(result.trim(), "");
    }

    #[test]
    fn test_dcs_payload_passthrough() {
        // Test cases for DCS payload passthrough behavior
        // Notes: body must be passed through verbatim.
        // - ESC '\' (ST) ends the string.
        // - ESC ESC stays as two bytes in the body.
        // - ESC X (X!='\') is data: both ESC and the following byte are payload.
        // - BEL (0x07) is data in DCS (not a terminator).

        let dcs_cases: &[(&[u8], &str)] = &[
            // 1) Minimal: embedded CSI SGR truecolor (colon params)
            (b"\x1bPq\x1b[38:2:12:34:56m\x1b\\", "<ESC>[38:2:12:34:56m"),
            // 2) Mixed payload: CSI + literal text
            (b"\x1bPq\x1b[48:2:0:0:0m;xyz\x1b\\", "<ESC>[48:2:0:0:0m;xyz"),
            // 3) DECRQSS-style reply payload (DCS 1$r ... ST) containing colon-CSI
            (
                b"\x1bP1$r\x1b[38:2:10:20:30;58:2::200:100:0m\x1b\\",
                "<ESC>[38:2:10:20:30;58:2::200:100:0m",
            ),
            // 4) ESC ESC and ESC X inside body (all data)
            (
                b"\x1bPqABC\x1b\x1bDEF\x1bXG\x1b\\",
                "ABC<ESC><ESC>DEF<ESC>XG",
            ),
            // 5) BEL in body (data, not a terminator)
            (b"\x1bPqDATA\x07MORE\x1b\\", "DATA<BEL>MORE"),
            // 6) iTerm2-style header (!|) with embedded CSI 256-color
            (b"\x1bP!|\x1b[38:5:208m\x1b\\", "<ESC>[38:5:208m"),
            // 7) Private prefix + final '|' (>|) with plain text payload
            (b"\x1bP>|Hello world\x1b\\", "Hello world"),
            // 8) Multiple embedded CSIs back-to-back
            (
                b"\x1bPq\x1b[38:2:1:2:3m\x1b[48:5:17m\x1b\\",
                "<ESC>[38:2:1:2:3m<ESC>[48:5:17m",
            ),
            // 9) Long colon param with leading zeros
            (
                b"\x1bPq\x1b[58:2::000:007:042m\x1b\\",
                "<ESC>[58:2::000:007:042m",
            ),
        ];

        for (input, expected_body) in dcs_cases {
            let events = collect_events(input);

            // Find DcsData events and concatenate their payloads
            let mut actual_body = String::new();
            for event in &events {
                if let Some(data_part) = event
                    .strip_prefix("DcsData('")
                    .and_then(|s| s.strip_suffix("')"))
                {
                    actual_body
                        .push_str(&data_part.replace("\x1b", "<ESC>").replace("\x07", "<BEL>"));
                }
            }

            assert_eq!(
                actual_body, *expected_body,
                "DCS payload mismatch for input {:?}. Full events: {:#?}",
                input, events
            );

            // Also verify we get proper DcsStart and DcsEnd events
            assert!(
                events.iter().any(|e| e.starts_with("DcsStart")),
                "Missing DcsStart for input {:?}. Events: {:#?}",
                input,
                events
            );
            assert!(
                events.iter().any(|e| e == "DcsEnd"),
                "Missing DcsEnd for input {:?}. Events: {:#?}",
                input,
                events
            );
        }
    }

    fn collect_events(input: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let mut p = VTPushParser::new();
        p.feed_with(input, &mut |ev| out.push(format!("{:?}", ev)));
        out
    }

    #[test]
    fn dcs_header_with_colon_is_ignored_case1() {
        // ESC P 1:2 q ... ST   -> colon inside header params (invalid)
        let ev = collect_events(b"\x1bP1:2qHELLO\x1b\\");
        // Expect: no DcsStart; the whole thing is ignored until ST
        assert!(ev.iter().all(|e| !e.starts_with("DcsStart")), "{ev:#?}");
    }

    #[test]
    fn dcs_header_with_colon_is_ignored_case2() {
        // Colon immediately after ESC P, before any digit
        let ev = collect_events(b"\x1bP:1qDATA\x1b\\");
        assert!(ev.iter().all(|e| !e.starts_with("DcsStart")), "{ev:#?}");
    }

    #[test]
    fn dcs_header_with_colon_is_ignored_case3() {
        // Mixed: digits;colon;digits then intermediates/final
        let ev = collect_events(b"\x1bP12:34!qPAYLOAD\x1b\\");
        assert!(ev.iter().all(|e| !e.starts_with("DcsStart")), "{ev:#?}");
    }

    #[test]
    fn osc_aborted_by_can_mid_body() {
        // ESC ] 0;Title <CAN> more <BEL>
        let mut s = Vec::new();
        s.extend_from_slice(b"\x1b]0;Title");
        s.push(CAN);
        s.extend_from_slice(b"more\x07");

        let ev = collect_debug(&s);

        // EXPECT_SPEC_STRICT: no events at all (no Start/Data/End)
        // assert!(ev.is_empty(), "{ev:#?}");

        // EXPECT_PUSH_PARSER: Start emitted, but NO Data, NO End
        assert!(ev.iter().any(|e| e.starts_with("OscStart")), "{ev:#?}");
        assert!(!ev.iter().any(|e| e.starts_with("OscData")), "{ev:#?}");
        assert!(!ev.iter().any(|e| e.starts_with("OscEnd")), "{ev:#?}");
    }

    #[test]
    fn osc_aborted_by_sub_before_terminator() {
        let mut s = Vec::new();
        s.extend_from_slice(b"\x1b]52;c;YWJjZA==");
        s.push(SUB); // abort
        s.extend_from_slice(b"\x1b\\"); // would have been ST, but must be ignored after abort

        let ev = collect_debug(&s);
        // SPEC-STRICT:
        // assert!(ev.is_empty(), "{ev:#?}");
        // PUSH-PARSER:
        assert!(ev.iter().any(|e| e.starts_with("OscStart")), "{ev:#?}");
        assert!(!ev.iter().any(|e| e.starts_with("OscData")), "{ev:#?}");
        assert!(!ev.iter().any(|e| e.starts_with("OscEnd")), "{ev:#?}");
    }

    /// Collect raw VTEvent debug lines for quick assertions.
    fn collect_debug(input: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let mut p = VTPushParser::new();
        p.feed_with(input, &mut |ev| out.push(format!("{:?}", ev)));
        out
    }

    #[test]
    fn dcs_aborted_by_can_before_body() {
        // ESC P q <CAN> ... ST
        let mut s = Vec::new();
        s.extend_from_slice(b"\x1bPq"); // header (valid: final 'q')
        s.push(CAN);
        s.extend_from_slice(b"IGNORED\x1b\\"); // should be raw

        let ev = collect_debug(&s);

        assert_eq!(ev.len(), 4, "{ev:#?}");
        assert_eq!(ev[0], "DcsStart(, '', q)");
        assert_eq!(ev[1], "DcsCancel");
        assert_eq!(ev[2], "Raw('IGNORED')");
        assert_eq!(ev[3], "Esc('', \\)");
    }

    #[test]
    fn dcs_aborted_by_can_mid_body() {
        // ESC P q ABC <CAN> more ST
        let mut s = Vec::new();
        s.extend_from_slice(b"\x1bPqABC");
        s.push(CAN);
        s.extend_from_slice(b"MORE\x1b\\"); // ignored after abort

        let ev = collect_debug(&s);

        assert_eq!(ev.len(), 4, "{ev:#?}");
        assert_eq!(ev[0], "DcsStart(, '', q)");
        assert_eq!(ev[1], "DcsCancel");
        assert_eq!(ev[2], "Raw('MORE')");
        assert_eq!(ev[3], "Esc('', \\)");
    }

    /* ========= SOS / PM / APC (ESC X, ESC ^, ESC _) ========= */

    #[test]
    fn spa_aborted_by_can_is_ignored() {
        // ESC _ data <CAN> more ST
        let mut s = Vec::new();
        s.extend_from_slice(b"\x1b_hello");
        s.push(CAN);
        s.extend_from_slice(b"world\x1b\\");

        let ev = collect_debug(&s);
        assert_eq!(ev.len(), 2, "{ev:#?}");
        assert_eq!(ev[0], "Raw('world')");
        assert_eq!(ev[1], "Esc('', \\)");
    }

    #[test]
    fn spa_sub_aborts_too() {
        let mut s = Vec::new();
        s.extend_from_slice(b"\x1bXhello");
        s.push(SUB);
        s.extend_from_slice(b"world\x1b\\");
        let ev = collect_debug(&s);
        assert_eq!(ev.len(), 2, "{ev:#?}");
        assert_eq!(ev[0], "Raw('world')");
        assert_eq!(ev[1], "Esc('', \\)");
    }

    /* ========= Sanity: CAN outside strings is a C0 EXECUTE ========= */

    #[test]
    fn can_in_ground_is_c0() {
        let mut s = Vec::new();
        s.extend_from_slice(b"abc");
        s.push(CAN);
        s.extend_from_slice(b"def");
        let ev = collect_debug(&s);
        // Expect Raw("abc"), C0(0x18), Raw("def")
        assert_eq!(ev.len(), 3, "{ev:#?}");
        assert_eq!(ev[0], "Raw('abc')");
        assert_eq!(ev[1], "C0(18)");
        assert_eq!(ev[2], "Raw('def')");
    }
}
