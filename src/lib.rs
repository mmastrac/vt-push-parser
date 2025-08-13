use smallvec::SmallVec;

const ESC: u8 = 0x1B;
const BEL: u8 = 0x07;
const DEL: u8 = 0x7F;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1A;
const ST_FINAL: u8 = b'\\';

#[derive(Default, Clone, Copy)]
pub struct VTIntermediate {
    data: [u8; 2],
}

impl VTIntermediate {
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

    // OSC stream
    OscStart,
    OscData(&'a [u8]),
    OscEnd {
        used_bel: bool,
    },
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
                    write!(f, ", {:02x?}", param)?;
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
    max_short_hdr: usize,
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
            max_short_hdr: 4096,
            stream_flush: 8192,
        }
    }

    pub fn with_limits(mut self, max_short_hdr: usize, stream_flush: usize) -> Self {
        self.max_short_hdr = max_short_hdr;
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
        // Emitting pending raw at end-of-chunk helps coalesce; you can remove if undesired.
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
            b'[' => {
                self.clear_hdr_collectors();
                self.st = CsiEntry;
            }
            b'P' => {
                self.clear_hdr_collectors();
                self.st = DcsEntry;
            }
            b']' => {
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
                self.st = Escape;
            }
            // stay until ST (handled in DcsEsc path if you want); we just drop here.
            _ => {}
        }
    }
    fn on_dcs_pass<F: FnMut(VTEvent)>(&mut self, b: u8, cb: &mut F) {
        use State::*;
        match b {
            CAN | SUB => {
                self.dcs_end(cb);
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
                self.dcs_put(ESC, cb); /* remain in DcsEsc */
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

    // ---- SOS/PM/APC (ignored payload as per your machine)
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
}
