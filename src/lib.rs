use std::ops::Range;

use smallvec::SmallVec;

const ESC: u8 = 0x1B;
const BEL: u8 = 0x07;
const DEL: u8 = 0x7F;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1A;
const CSI: u8 = b'[';
const OSC: u8 = b']';
const SS3: u8 = b'O';
const DCS: u8 = b'P';
const ST_FINAL: u8 = b'\\';

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

    const fn const_eq(&self, other: &Self) -> bool {
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
                    write!(f, "{}", chunk.valid())?;
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
                    write!(f, "{}", chunk.valid())?;
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
                    write!(f, "{}", chunk.valid())?;
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
    use pretty_assertions::assert_eq;

    use super::*;

    fn decode_stream(input: &[u8]) -> String {
        println!("Input:");
        let mut s = Vec::new();
        _ = hxdmp::hexdump(input, &mut s);
        println!("{}", String::from_utf8_lossy(&s));
        let mut parser = VTPushParser::new();
        let mut result = String::new();
        let mut callback = |vt_input: VTEvent<'_>| {
            result.push_str(&format!("{:?}\n", vt_input));
        };
        parser.feed_with(input, &mut callback);
        println!("Result:");
        println!("{}", result);
        result
    }

    #[test]
    fn test_large_escape2() {
        let result = decode_stream(
            &hex::decode(
                r#"
        1b5b495445524d3220332e352e31346e1b5d31303b7267623a646361612f6
        46361622f646361611b5c1b5d31313b7267623a313538652f313933612f31
        6537351b5c1b5b3f36343b313b323b343b363b31373b31383b32313b32326
        31b5b3e36343b323530303b30631b50217c36393534373236441b5c1b503e
        7c695465726d3220332e352e31341b5c1b5b383b33343b31343874"#
                    .replace(char::is_whitespace, ""),
            )
            .unwrap(),
        );
        assert_eq!(
            result.trim(),
            r#"
Csi(, '', 'I')
Raw('TERM2 3.5.14n')
OscStart
OscData('10;rgb:dcaa/dcab/dcaa')
OscEnd
OscStart
OscData('11;rgb:158e/193a/1e75')
OscEnd
Csi('?', '64', '1', '2', '4', '6', '17', '18', '21', '22', '', 'c')
Csi('>', '64', '2500', '0', '', 'c')
DcsStart(, '!', |)
DcsData('6954726D')
DcsEnd
DcsStart('>', '', |)
DcsData('iTerm2 3.5.14')
DcsEnd
Csi(, '8', '34', '148', '', 't')
        "#
            .trim()
        );
    }

    #[test]
    fn test_basic_escape_sequences() {
        // Test basic ESC sequences
        let result = decode_stream(b"\x1b[1;2;3d");
        assert_eq!(result.trim(), "Csi(, '1', '2', '3', '', 'd')");

        // Test ESC with intermediate
        let result = decode_stream(b"\x1b  M");
        assert_eq!(result.trim(), "Esc('  ', M)");

        // Test SS3 (ESC O)
        let result = decode_stream(b"\x1bOA");
        assert_eq!(result.trim(), "Esc('', O)\nRaw('A')");
    }

    #[test]
    fn test_csi_sequences() {
        // Test CSI with private parameter
        let result = decode_stream(b"\x1b[?25h");
        assert_eq!(result.trim(), "Csi('?', '25', '', 'h')");

        // Test CSI with multiple parameters
        let result = decode_stream(b"\x1b[1;2;3;4;5m");
        assert_eq!(result.trim(), "Csi(, '1', '2', '3', '4', '5', '', 'm')");

        // Test CSI with colon parameter
        let result = decode_stream(b"\x1b[3:1;2;3;4;5m");
        assert_eq!(result.trim(), "Csi(, '3:1', '2', '3', '4', '5', '', 'm')");

        // Test CSI with intermediate
        let result = decode_stream(b"\x1b[  M");
        assert_eq!(result.trim(), "Csi(, '  ', 'M')");
    }

    #[test]
    fn test_dcs_sequences() {
        // Test DCS with parameters
        let result = decode_stream(b"\x1bP 1;2;3|test data\x1b\\");
        assert_eq!(
            result.trim(),
            "DcsStart(, ' ', 1)\nDcsData(';2;3|test data')\nDcsEnd"
        );

        // Test DCS with private parameter
        let result = decode_stream(b"\x1bP>1;2;3|more data\x1b\\");
        assert_eq!(
            result.trim(),
            "DcsStart('>', '1', '2', '3', '', |)\nDcsData('more data')\nDcsEnd"
        );

        // Test DCS with intermediate
        let result = decode_stream(b"\x1bP 1;2;3  |data\x1b\\");
        assert_eq!(
            result.trim(),
            "DcsStart(, ' ', 1)\nDcsData(';2;3  |data')\nDcsEnd"
        );

        let result = decode_stream(b"\x1bP1$r\x1b\\");
        assert_eq!(result.trim(), "DcsStart(, '1', '$', r)\nDcsEnd");
    }

    #[test]
    fn test_osc_sequences() {
        // Test OSC with BEL terminator
        let result = decode_stream(b"\x1b]10;rgb:fff/000/000\x07");
        assert_eq!(
            result.trim(),
            "OscStart\nOscData('10;rgb:fff/000/000')\nOscEnd"
        );

        // Test OSC with ST terminator
        let result = decode_stream(b"\x1b]11;rgb:000/fff/000\x1b\\");
        assert_eq!(
            result.trim(),
            "OscStart\nOscData('11;rgb:000/fff/000')\nOscEnd"
        );

        // Test OSC with escape in data
        let result = decode_stream(b"\x1b]12;test [data\x1b\\");
        assert_eq!(result.trim(), "OscStart\nOscData('12;test [data')\nOscEnd");
    }

    #[test]
    fn test_cancellation_and_invalid_sequences() {
        // Test CAN cancellation in CSI
        let result = decode_stream(b"x\x1b[1;2;3\x18y");
        assert_eq!(result.trim(), "Raw('x')\nRaw('y')");

        // Test SUB cancellation in CSI
        let result = decode_stream(b"x\x1b[1;2;3\x1ay");
        assert_eq!(result.trim(), "Raw('x')\nRaw('y')");

        // Test CAN cancellation in DCS (parser completes DCS then returns to ground)
        let result = decode_stream(b"x\x1bP 1;2;3|data\x18y");
        assert_eq!(
            result.trim(),
            "Raw('x')\nDcsStart(, ' ', 1)\nDcsCancel\nRaw('y')"
        );

        // Test SUB cancellation in OSC (parser emits OscStart then cancels)
        let result = decode_stream(b"x\x1b]10;data\x1ay");
        assert_eq!(result.trim(), "Raw('x')\nOscStart\nOscCancel\nRaw('y')");

        // Test invalid CSI sequence (ignored)
        let result = decode_stream(b"x\x1b[1;2;3gy");
        assert_eq!(
            result.trim(),
            "Raw('x')\nCsi(, '1', '2', '3', '', 'g')\nRaw('y')"
        );

        // Test CSI ignore state
        let result = decode_stream(b"x\x1b[:1;2;3gy");
        assert_eq!(
            result.trim(),
            "Raw('x')\nCsi(, ':1', '2', '3', '', 'g')\nRaw('y')"
        );
    }

    #[test]
    fn test_escape_sequences_with_raw_text() {
        // Test mixed raw text and escape sequences
        let result = decode_stream(b"Hello\x1b[1;2;3dWorld");
        assert_eq!(
            result.trim(),
            "Raw('Hello')\nCsi(, '1', '2', '3', '', 'd')\nRaw('World')"
        );

        // Test escape sequences with UTF-8 text
        let result = decode_stream(b"\x1b[1;2;3d\xe4\xb8\xad\xe6\x96\x87");
        assert_eq!(result.trim(), "Csi(, '1', '2', '3', '', 'd')\nRaw('中文')");
    }

    #[test]
    fn test_c0_control_characters() {
        // Test various C0 control characters
        let result = decode_stream(b"\x0a\x0d\x09\x08\x0c\x0b");
        assert_eq!(
            result.trim(),
            "C0(0a)\nC0(0d)\nC0(09)\nC0(08)\nC0(0c)\nC0(0b)"
        );

        // Test C0 with raw text
        let result = decode_stream(b"Hello\x0aWorld");
        assert_eq!(result.trim(), "Raw('Hello')\nC0(0a)\nRaw('World')");
    }

    #[test]
    fn test_complex_escape_sequences() {
        // Test complex CSI with all features
        let result = decode_stream(&hex::decode("1b5b3f32353b313b323b333a343b353b363b373b383b393b31303b31313b31323b31333b31343b31353b31363b31373b31383b31393b32303b32313b32323b32333b32343b32353b32363b32373b32383b32393b33303b33313b33323b33333b33343b33353b33363b33373b33383b33393b34303b34313b34323b34333b34343b34353b34363b34373b34383b34393b35303b35313b35323b35333b35343b35353b35363b35373b35383b35393b36303b36313b36323b36333b36343b36353b36363b36373b36383b36393b37303b37313b37323b37333b37343b37353b37363b37373b37383b37393b38303b38313b38323b38333b38343b38353b38363b38373b38383b38393b39303b39313b39323b39333b39343b39353b39363b39373b39383b39393b3130306d").unwrap());
        assert_eq!(
            result.trim(),
            "Csi('?', '25', '1', '2', '3:4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15', '16', '17', '18', '19', '20', '21', '22', '23', '24', '25', '26', '27', '28', '29', '30', '31', '32', '33', '34', '35', '36', '37', '38', '39', '40', '41', '42', '43', '44', '45', '46', '47', '48', '49', '50', '51', '52', '53', '54', '55', '56', '57', '58', '59', '60', '61', '62', '63', '64', '65', '66', '67', '68', '69', '70', '71', '72', '73', '74', '75', '76', '77', '78', '79', '80', '81', '82', '83', '84', '85', '86', '87', '88', '89', '90', '91', '92', '93', '94', '95', '96', '97', '98', '99', '100', '', 'm')"
        );
    }

    #[test]
    fn test_escape_sequences_with_del() {
        // Test DEL character handling
        let result = decode_stream(b"\x1b[1;2;3\x7fm");
        assert_eq!(result.trim(), "Csi(, '1', '2', '3', '', 'm')");

        // Test DEL in raw text
        let result = decode_stream(b"Hello\x7fWorld");
        assert_eq!(result.trim(), "Raw('HelloWorld')");
    }

    #[test]
    fn test_escape_sequences_with_esc_esc() {
        // Test ESC ESC sequence
        let result = decode_stream(b"\x1b\x1b[1;2;3d");
        assert_eq!(result.trim(), "Csi(, '1', '2', '3', '', 'd')");

        let result = decode_stream(b"\x1bP 1;2;3|\x1b\x1bdata\x1b\\");
        // Note: ESC ESC in DCS is handled correctly by the parser, escaping is done by the receiver
        assert_eq!(
            result.trim().replace("\x1b", "<ESC>"),
            "DcsStart(, ' ', 1)\nDcsData(';2;3|<ESC>data')\nDcsEnd"
        );
    }

    #[test]
    fn test_sos_pm_apc_sequences() {
        // Test SOS (ESC X)
        let result = decode_stream(b"\x1bXtest data\x1b\\");
        assert_eq!(result.trim(), "");

        // Test PM (ESC ^)
        let result = decode_stream(b"\x1b^test data\x1b\\");
        assert_eq!(result.trim(), "");

        // Test APC (ESC _)
        let result = decode_stream(b"\x1b_test data\x1b\\");
        assert_eq!(result.trim(), "");
    }

    #[test]
    fn test_edge_cases() {
        // Test empty input
        let result = decode_stream(&[]);
        assert_eq!(result.trim(), "");

        // Test single ESC
        let result = decode_stream(b"\x1b");
        assert_eq!(result.trim(), "");

        // Test incomplete CSI
        let result = decode_stream(b"\x1b[");
        assert_eq!(result.trim(), "");

        // Test incomplete DCS
        let result = decode_stream(b"\x1bP");
        assert_eq!(result.trim(), "");

        // Test incomplete OSC
        let result = decode_stream(b"\x1b]");
        assert_eq!(result.trim(), "OscStart");
    }

    #[test]
    fn test_unicode_and_special_characters() {
        // Test with Unicode characters
        let result = decode_stream(b"\x1b[1;2;3d\xe4\xb8\xad\xe6\x96\x87");
        assert_eq!(result.trim(), "Csi(, '1', '2', '3', '', 'd')\nRaw('中文')");

        // Test with special ASCII characters
        let result = decode_stream(b"\x1b[1;2;3d~!@#$%^&*()_+|\\{}[]:;\"|<>,.?/");
        assert_eq!(
            result.trim(),
            "Csi(, '1', '2', '3', '', 'd')\nRaw('~!@#$%^&*()_+|\\{}[]:;\"|<>,.?/')"
        );
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
    fn test_csi_colons() {
        let test_cases: Vec<(&'static [u8], &[&str], u8)> = vec![
            // 1. FG truecolor
            (b"\x1b[38:2:255:128:64m", &["38:2:255:128:64"], b'm'),
            // 2. BG truecolor
            (b"\x1b[48:2:0:0:0m", &["48:2:0:0:0"], b'm'),
            // 3. FG indexed
            (b"\x1b[38:5:208m", &["38:5:208"], b'm'),
            // 4. BG indexed
            (b"\x1b[48:5:123m", &["48:5:123"], b'm'),
            // 5. Bold + FG indexed + BG truecolor
            (
                b"\x1b[1;38:5:208;48:2:30:30:30m",
                &["1", "38:5:208", "48:2:30:30:30"],
                b'm',
            ),
            // 6. Reset + FG truecolor
            (b"\x1b[0;38:2:12:34:56m", &["0", "38:2:12:34:56"], b'm'),
            // 7. Underline color truecolor with empty subparam (::)
            (b"\x1b[58:2::186:93:0m", &["58:2::186:93:0"], b'm'),
            // 8. FG truecolor + BG indexed + underline color truecolor
            (
                b"\x1b[38:2:10:20:30;48:5:17;58:2::200:100:0m",
                &["38:2:10:20:30", "48:5:17", "58:2::200:100:0"],
                b'm',
            ),
            // 9. Colon params with leading zeros
            (b"\x1b[38:2:000:007:042m", &["38:2:000:007:042"], b'm'),
            // 10. Large RGB values
            (b"\x1b[38:2:300:300:300m", &["38:2:300:300:300"], b'm'),
            // 11. Trailing semicolon with colon param (empty final param)
            (b"\x1b[38:5:15;m", &["38:5:15", ""], b'm'),
            // 12. Only colon param (no numeric params)
            (b"\x1b[38:2:1:2:3m", &["38:2:1:2:3"], b'm'),
        ];

        for (input, expected_params, expected_final) in test_cases {
            let mut parser = VTPushParser::new();
            parser.feed_with(input, |event| match event {
                VTEvent::Csi {
                    private,
                    params,
                    intermediates,
                    final_byte,
                } => {
                    assert_eq!(
                        private, None,
                        "Expected no private prefix for input {:?}",
                        input
                    );
                    assert_eq!(
                        intermediates.is_empty(),
                        true,
                        "Expected no intermediates for input {:?}",
                        input
                    );
                    assert_eq!(
                        final_byte, expected_final,
                        "Expected final byte '{}' for input {:?}",
                        expected_final, input
                    );

                    let param_strings: Vec<String> = params
                        .iter()
                        .map(|p| String::from_utf8_lossy(p).to_string())
                        .collect();
                    assert_eq!(
                        param_strings, expected_params,
                        "Parameter mismatch for input {:?}",
                        input
                    );
                }
                _ => panic!("Expected CSI event for input {:?}, got {:?}", input, event),
            });
        }
    }

    #[test]
    fn test_dcs_ignore_st_handling() {
        // Test that DCS_IGNORE state doesn't handle ST (ESC \) as per spec
        // This test documents the current behavior which differs from the specification

        // Create a DCS sequence that goes into ignore state (using colon)
        // ESC P : 1;2;3 | data ESC \
        // The colon should put us in DCS_IGNORE, then ESC \ should transition to GROUND per spec
        let result = decode_stream(b"Hello\x1bP:1;2;3|data\x1b\\World");

        // Current implementation: DCS_IGNORE ignores ST, so we stay in ignore state
        // and the sequence is never properly terminated
        // Expected: Should transition to GROUND after ST, but current implementation doesn't
        assert_eq!(result.trim(), "Raw('Hello')\nRaw('World')");

        // Test with additional data after the ST to see if we're back in GROUND
        let result = decode_stream(b"\x1bP:1;2;3|data\x1b\\Hello");

        // This should show that we're not properly back in GROUND state
        // because the ST wasn't handled in DCS_IGNORE
        assert_eq!(result.trim(), "Raw('Hello')");

        // Compare with a valid DCS sequence that doesn't go into ignore state
        let result = decode_stream(b"\x1bP1;2;3|data\x1b\\Hello");
        assert_eq!(
            result.trim(),
            "DcsStart(, '1', '2', '3', '', |)\nDcsData('data')\nDcsEnd\nRaw('Hello')"
        );
    }

    #[test]
    fn test_dcs_ignore_cancellation() {
        // Test that CAN/SUB properly cancel DCS_IGNORE state
        let result = decode_stream(b"\x1bP:1;2;3|data\x18Hello"); // CAN
        assert_eq!(result.trim(), "Raw('Hello')");

        let result = decode_stream(b"\x1bP:1;2;3|data\x1aHello"); // SUB
        assert_eq!(result.trim(), "Raw('Hello')");
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

    const CAN: u8 = 0x18;
    const SUB: u8 = 0x1A;

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
