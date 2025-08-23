pub mod event;
pub mod signature;

use smallvec::SmallVec;

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

macro_rules! ascii_control {
    ($(($variant:ident, $value:expr)),* $(,)?) => {
        /// ASCII control codes.
        #[repr(u8)]
        pub enum AsciiControl {
            $( $variant = $value, )*
        }

        impl std::fmt::Display for AsciiControl {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( AsciiControl::$variant => write!(f, "<{}>", stringify!($variant).to_ascii_uppercase()), )*
                }
            }
        }

        impl std::fmt::Debug for AsciiControl {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( AsciiControl::$variant => write!(f, "<{}>", stringify!($variant).to_ascii_uppercase()), )*
                }
            }
        }

        impl TryFrom<u8> for AsciiControl {
            type Error = ();
            fn try_from(value: u8) -> Result<Self, Self::Error> {
                $(
                    if value == $value {
                        return Ok(AsciiControl::$variant);
                    }
                )*
                Err(())
            }
        }

        impl TryFrom<char> for AsciiControl {
            type Error = ();
            fn try_from(value: char) -> Result<Self, Self::Error> {
                $(
                    if value == char::from($value) {
                        return Ok(AsciiControl::$variant);
                    }
                )*
                Err(())
            }
        }

        impl std::str::FromStr for AsciiControl {
            type Err = ();
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $(
                    if s.eq_ignore_ascii_case(stringify!($name)) {
                        return Ok(AsciiControl::$variant);
                    }
                )*
                Err(())
            }
        }
    };
}

ascii_control! {
    (Nul, 0),
    (Soh, 1),
    (Stx, 2),
    (Etx, 3),
    (Eot, 4),
    (Enq, 5),
    (Ack, 6),
    (Bel, 7),
    (Bs, 8),
    (Tab, 9),
    (Lf, 10),
    (Vt, 11),
    (Ff, 12),
    (Cr, 13),
    (So, 14 ),
    (Si, 15),
    (Dle, 16),
    (Dc1, 17),
    (Dc2, 18),
    (Dc3, 19),
    (Dc4, 20),
    (Nak, 21),
    (Syn, 22),
    (Etb, 23),
    (Can, 24),
    (Em, 25),
    (Sub, 26),
    (Esc, 27),
    (Fs, 28),
    (Gs, 29),
    (Rs, 30),
    (Us, 31),
    (Del, 127),
}

// Re-export the main types for backward compatibility
pub use event::{VTEvent, VTIntermediate};
pub use signature::VTEscapeSignature;

/// The action to take with the most recently accumulated byte.
pub enum VTAction<'a> {
    /// The parser will accumulate the byte and continue processing.
    None,
    /// The parser emitted an event.
    Event(VTEvent<'a>),
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

    // GROUND raw coalescing
    raw_buf: Vec<u8>,

    // Header collectors for short escapes (we borrow from these in callbacks)
    ints: VTIntermediate,
    params: Vec<Vec<u8>>,
    cur_param: Vec<u8>,
    priv_prefix: Option<u8>,

    // Streaming buffer (DCS/OSC bodies)
    stream_buf: Vec<u8>,
    used_bel: bool,

    // Limits
    stream_flush: usize,
}

impl VTPushParser {
    pub fn new() -> Self {
        Self {
            st: State::Ground,
            raw_buf: Vec::with_capacity(256),
            ints: VTIntermediate::default(),
            params: Vec::with_capacity(8),
            cur_param: Vec::with_capacity(8),
            priv_prefix: None,
            stream_buf: Vec::with_capacity(8192),
            used_bel: false,
            stream_flush: 8192,
        }
    }

    /// Decode a buffer of bytes into a series of events.
    pub fn decode_buffer<'a>(input: &'a [u8], mut cb: impl for<'b> FnMut(VTEvent<'b>)) {
        let mut parser = Self::new();
        for &b in input {
            parser.push_with(b, &mut cb);
        }
        parser.flush_raw_if_any(&mut cb);
        parser.finish(&mut cb);
    }

    pub fn with_stream_flush_max(mut self, stream_flush: usize) -> Self {
        self.stream_flush = stream_flush.max(1);
        self
    }

    /* =====================
    Callback-driven API
    ===================== */

    pub fn feed_with<F: FnMut(VTEvent)>(&mut self, input: &[u8], mut cb: F) {
        for &b in input {
            self.push_with(b, &mut cb);
        }
        self.flush_raw_if_any(&mut cb);
    }

    pub fn push_with<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match self.st {
            Ground => self.on_ground(b, cb),
            Escape => self.on_escape(b, cb),
            EscInt => self.on_esc_int(b, cb),

            CsiEntry => self.on_csi_entry(b, cb),
            CsiParam => self.on_csi_param(b, cb),
            CsiInt => self.on_csi_int(b, cb),
            CsiIgnore => self.on_csi_ignore(b, cb),

            DcsEntry => self.on_dcs_entry(b, cb),
            DcsParam => self.on_dcs_param(b, cb),
            DcsInt => self.on_dcs_int(b, cb),
            DcsIgnore => self.on_dcs_ignore(b, cb),
            DcsIgnoreEsc => self.on_dcs_ignore_esc(b, cb),
            DcsPassthrough => self.on_dcs_pass(b, cb),
            DcsEsc => self.on_dcs_esc(b, cb),

            OscString => self.on_osc_string(b, cb),
            OscEsc => self.on_osc_esc(b, cb),

            SosPmApcString => self.on_spa_string(b, cb),
            SpaEsc => self.on_spa_esc(b, cb),
        }
    }

    pub fn finish<F: FnMut(VTEvent)>(&mut self, cb: &mut F) {
        // Abort unterminated strings and flush raw.
        self.reset_collectors();
        self.st = State::Ground;
        self.flush_raw_if_any(cb);
    }

    /* =====================
    Emit helpers (borrowed)
    ===================== */

    fn flush_raw_if_any<F: FnMut(VTEvent)>(&mut self, cb: &mut F) {
        if !self.raw_buf.is_empty() {
            let slice = &self.raw_buf[..];
            cb(VTEvent::Raw(slice));
            self.raw_buf.clear(); // borrow ended with callback return
        }
    }

    fn clear_hdr_collectors(&mut self) {
        self.ints.clear();
        self.params.clear();
        self.cur_param.clear();
        self.priv_prefix = None;
    }

    fn reset_collectors(&mut self) {
        self.clear_hdr_collectors();
        self.stream_buf.clear();
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

    fn emit_esc<F: FnMut(VTEvent)>(&mut self, final_byte: u8, cb: &mut F) {
        self.flush_raw_if_any(cb);
        cb(VTEvent::Esc {
            intermediates: self.ints,
            final_byte,
        });
        self.clear_hdr_collectors();
    }

    fn emit_csi<F: FnMut(VTEvent)>(&mut self, final_byte: u8, cb: &mut F) {
        self.flush_raw_if_any(cb);
        self.finish_params_if_any();

        // Build borrowed views into self.params
        let mut borrowed: SmallVec<[&[u8]; 4]> = SmallVec::new();
        borrowed.extend(self.params.iter().map(|v| v.as_slice()));

        let privp = self.priv_prefix.take();
        cb(VTEvent::Csi {
            private: privp,
            params: borrowed,
            intermediates: self.ints,
            final_byte,
        });
        self.clear_hdr_collectors();
    }

    fn dcs_start<F: FnMut(VTEvent)>(&mut self, final_byte: u8, cb: &mut F) {
        self.flush_raw_if_any(cb);
        self.finish_params_if_any();

        let mut borrowed: SmallVec<[&[u8]; 4]> = SmallVec::new();
        borrowed.extend(self.params.iter().map(|v| v.as_slice()));

        let privp = self.priv_prefix.take();
        cb(VTEvent::DcsStart {
            priv_prefix: privp,
            params: borrowed,
            intermediates: self.ints,
            final_byte,
        });
        self.stream_buf.clear();
        // keep header buffers intact until after callback; already done
        self.clear_hdr_collectors();
    }

    fn dcs_put<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        self.stream_buf.push(b);
        if self.stream_buf.len() >= self.stream_flush {
            let slice = &self.stream_buf[..];
            cb(VTEvent::DcsData(slice));
            self.stream_buf.clear();
        }
    }

    fn dcs_end<F: FnMut(VTEvent)>(&mut self, cb: &mut F) {
        if !self.stream_buf.is_empty() {
            let slice = &self.stream_buf[..];
            cb(VTEvent::DcsData(slice));
            self.stream_buf.clear();
        }
        cb(VTEvent::DcsEnd);
        self.reset_collectors();
    }

    fn osc_start<F: FnMut(VTEvent)>(&mut self, cb: &mut F) {
        self.flush_raw_if_any(cb);
        self.used_bel = false;
        self.stream_buf.clear();
        cb(VTEvent::OscStart);
    }

    fn osc_put<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        self.stream_buf.push(b);
        if self.stream_buf.len() >= self.stream_flush {
            let slice = &self.stream_buf[..];
            cb(VTEvent::OscData(slice));
            self.stream_buf.clear();
        }
    }

    fn osc_end<F: FnMut(VTEvent)>(&mut self, cb: &mut F) {
        if !self.stream_buf.is_empty() {
            let slice = &self.stream_buf[..];
            cb(VTEvent::OscData(slice));
            self.stream_buf.clear();
        }
        let used_bel = self.used_bel;
        cb(VTEvent::OscEnd { used_bel });
        self.reset_collectors();
    }

    /* =====================
    State handlers
    ===================== */

    fn on_ground<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        match b {
            ESC => {
                self.clear_hdr_collectors();
                self.flush_raw_if_any(cb);
                self.st = State::Escape;
            }
            DEL => {}
            c if is_c0(c) => {
                self.flush_raw_if_any(cb);
                cb(VTEvent::C0(c));
            }
            p if is_printable(p) => {
                self.raw_buf.push(p);
            }
            _ => {
                self.raw_buf.push(b);
            } // safe fallback
        }
    }

    fn on_escape<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = EscInt;
            }
            CSI => {
                self.clear_hdr_collectors();
                self.st = CsiEntry;
            }
            DCS => {
                self.clear_hdr_collectors();
                self.st = DcsEntry;
            }
            OSC => {
                self.clear_hdr_collectors();
                self.osc_start(cb);
                self.st = OscString;
            }
            b'X' | b'^' | b'_' => {
                self.clear_hdr_collectors();
                self.st = State::SosPmApcString;
            }
            c if is_final(c) => {
                self.emit_esc(c, cb);
                self.st = Ground;
            }
            ESC => { /* ESC ESC allowed */ }
            _ => {
                self.st = Ground;
            }
        }
    }
    fn on_esc_int<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            c if is_intermediate(c) => {
                self.ints.push(c);
            }
            c if is_final(c) => {
                self.emit_esc(c, cb);
                self.st = Ground;
            }
            _ => {
                self.st = Ground;
            }
        }
    }

    // ---- CSI
    fn on_csi_entry<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = Escape;
            }
            c if is_priv(c) => {
                self.priv_prefix = Some(c);
                self.st = CsiParam;
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
                self.st = CsiParam;
            }
            b';' => {
                self.next_param();
                self.st = CsiParam;
            }
            b':' => {
                self.cur_param.push(b':');
                self.st = CsiParam;
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = CsiInt;
            }
            c if is_final(c) => {
                self.emit_csi(c, cb);
                self.st = Ground;
            }
            _ => {
                self.st = CsiIgnore;
            }
        }
    }
    fn on_csi_param<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = Escape;
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
            }
            b';' => {
                self.next_param();
            }
            b':' => {
                self.cur_param.push(b':');
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = CsiInt;
            }
            c if is_final(c) => {
                self.emit_csi(c, cb);
                self.st = Ground;
            }
            _ => {
                self.st = CsiIgnore;
            }
        }
    }
    fn on_csi_int<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = Escape;
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
            }
            c if is_final(c) => {
                self.emit_csi(c, cb);
                self.st = Ground;
            }
            _ => {
                self.st = CsiIgnore;
            }
        }
    }
    fn on_csi_ignore<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = Escape;
            }
            c if is_final(c) => {
                self.st = Ground;
            }
            _ => {}
        }
    }

    // ---- DCS
    fn on_dcs_entry<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = Escape;
            }
            c if is_priv(c) => {
                self.priv_prefix = Some(c);
                self.st = DcsParam;
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
                self.st = DcsParam;
            }
            b';' => {
                self.next_param();
                self.st = DcsParam;
            }
            b':' => {
                self.st = DcsIgnore;
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = DcsInt;
            }
            c if is_final(c) => {
                self.dcs_start(c, cb);
                self.st = DcsPassthrough;
            }
            _ => {
                self.st = DcsIgnore;
            }
        }
    }
    fn on_dcs_param<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = Escape;
            }
            d if is_digit(d) => {
                self.cur_param.push(d);
            }
            b';' => {
                self.next_param();
            }
            b':' => {
                self.st = DcsIgnore;
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
                self.st = DcsInt;
            }
            c if is_final(c) => {
                self.dcs_start(c, cb);
                self.st = DcsPassthrough;
            }
            _ => {
                self.st = DcsIgnore;
            }
        }
    }
    fn on_dcs_int<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = Escape;
            }
            c if is_intermediate(c) => {
                self.ints.push(c);
            }
            c if is_final(c) => {
                self.dcs_start(c, cb);
                self.st = DcsPassthrough;
            }
            _ => {
                self.st = DcsIgnore;
            }
        }
    }
    fn on_dcs_ignore<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = DcsIgnoreEsc;
            }
            _ => {}
        }
    }
    fn on_dcs_ignore_esc<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
            }
            ST_FINAL => {
                self.st = Ground;
            }
            DEL => {}
            _ => {
                self.st = DcsIgnore;
            }
        }
    }
    fn on_dcs_pass<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                cb(VTEvent::DcsCancel);
                self.st = Ground;
            }
            DEL => {}
            ESC => {
                self.st = DcsEsc;
            }
            _ => {
                self.dcs_put(b, cb);
            }
        }
    }
    fn on_dcs_esc<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            ST_FINAL => {
                self.dcs_end(cb);
                self.st = Ground;
            } // ST
            ESC => {
                self.dcs_put(ESC, cb);
                self.st = DcsPassthrough;
            }
            _ => {
                self.dcs_put(ESC, cb);
                self.dcs_put(b, cb);
                self.st = DcsPassthrough;
            }
        }
    }

    // ---- OSC
    fn on_osc_string<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.reset_collectors();
                cb(VTEvent::OscCancel);
                self.st = Ground;
            }
            DEL => {}
            BEL => {
                self.used_bel = true;
                self.osc_end(cb);
                self.st = Ground;
            }
            ESC => {
                self.st = State::OscEsc;
            }
            p if is_printable(p) => {
                self.osc_put(p, cb);
            }
            _ => {} // ignore other C0
        }
    }
    fn on_osc_esc<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            ST_FINAL => {
                self.used_bel = false;
                self.osc_end(cb);
                self.st = Ground;
            } // ST
            ESC => {
                self.osc_put(ESC, cb); /* remain in OscEsc */
            }
            _ => {
                self.osc_put(ESC, cb);
                self.osc_put(b, cb);
                self.st = OscString;
            }
        }
    }

    // ---- SOS/PM/APC (ignored payload)
    fn on_spa_string<F: FnMut(VTEvent)>(&mut self, b: u8, _cb: &mut F) {
        match b {
            CAN | SUB => {
                self.reset_collectors();
                self.st = State::Ground;
            }
            DEL => {}
            ESC => {
                self.st = State::SpaEsc;
            }
            _ => {}
        }
    }
    fn on_spa_esc<F: FnMut(VTEvent)>(&mut self, b: u8, _cb: &mut F) {
        match b {
            ST_FINAL => {
                self.reset_collectors();
                self.st = State::Ground;
            }
            ESC => { /* remain */ }
            _ => {
                self.st = State::SosPmApcString;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::OpenOptions, sync::Mutex};

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
        let mut parser = VTPushParser::new().with_stream_flush_max(4); // Small flush size
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
            "DcsStart(, '1', '2', '3', ' ', |)\nDcsData('data')\nDcsData(' mor')\nDcsData('e')\nDcsEnd"
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
            (b"\x1bPqABC\x1b\x1bDEF\x1bXG\x1b\\", "ABC<ESC>DEF<ESC>XG"),
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
            // 10) Payload that includes a literal ST *text* sequence "ESC \"
            //     (Note: inside DCS, this would actually TERMINATE; so to keep it as data we send "ESC ESC '\\"
            (
                b"\x1bPqKEEP \x1b\x1b\\ LITERAL ST\x1b\\",
                "KEEP <ESC>\\ LITERAL ST",
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
        p.feed_with(input, |ev| out.push(format!("{:?}", ev)));
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
        p.feed_with(input, |ev| out.push(format!("{:?}", ev)));
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
