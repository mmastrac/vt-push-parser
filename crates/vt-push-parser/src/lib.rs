//! A streaming push parser for the VT/xterm protocol.
//!
//! Use [`VTPushParser::feed_with`] to feed bytes into the parser, handling the
//! [`VTEvent`]s as they are emitted.
//!
//! ```rust
//! use vt_push_parser::VTPushParser;
//!
//! let mut parser = VTPushParser::new();
//! let mut output = String::new();
//! parser.feed_with(b"\x1b[32mHello, world!\x1b[0m", &mut |event| {
//!     output.push_str(&format!("{:?}", event));
//! });
//! assert_eq!(output, "Csi(, '32', '', 'm')Raw('Hello, world!')Csi(, '0', '', 'm')");
//! ```
//!
//! ## Interest
//!
//! The parser can be configured to only emit certain types of events by setting
//! the `INTEREST` parameter. Other event types will be parsed and discarded.
//!
//! For example, to only emit CSI (and Raw) events:
//!
//! ```rust
//! use vt_push_parser::{VTPushParser, VT_PARSER_INTEREST_CSI};
//!
//! let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_CSI>();
//! ```
//!
//! ## Input parsing
//!
//! This crate is designed to be used for parsing terminal output, but it can
//! also be used for parsing input. Input is not always well-formed, however and
//! may contain mode-switching escapes that require the parser to turn off its
//! normal parsing behaviours (ie: bracketed-paste mode, xterm mouse events,
//! etc).
//!
//! The [`capture::VTCapturePushParser`] is useful for parsing input that may
//! work in this way.
pub mod ascii;
pub mod capture;
pub mod event;
pub mod iter;
pub mod signature;

use smallvec::SmallVec;

use ascii::AsciiControl;
use event::{CSI, DCS, Esc, EscInvalid, SS2, SS3, VTEvent, VTIntermediate};

const ESC: u8 = AsciiControl::Esc as _;
const BEL: u8 = AsciiControl::Bel as _;
const DEL: u8 = AsciiControl::Del as _;
const CAN: u8 = AsciiControl::Can as _;
const SUB: u8 = AsciiControl::Sub as _;
const CSI: u8 = b'[';
const OSC: u8 = b']';
const SS2: u8 = b'N';
const SS3: u8 = b'O';
const DCS: u8 = b'P';
const APC: u8 = b'_';
const PM: u8 = b'^';
const SOS: u8 = b'X';
const ST_FINAL: u8 = b'\\';

use crate::event::{Param, ParamBuf, Params};

/// The action to take with the most recently accumulated byte.
enum VTAction<'a> {
    /// The parser will accumulate the byte and continue processing. If
    /// currently buffered, emit the buffered bytes.
    None,
    /// The parser emitted an event. If currently buffered, emit the buffered
    /// bytes.
    Event(VTEvent<'a>),
    /// The parser ended a region.
    End(VTEnd),
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
enum VTEmit {
    /// Emit this byte as a ground-state character.
    Ground,
    /// Emit this byte into the current DCS stream.
    Dcs,
    /// Emit this byte into the current OSC stream.
    Osc,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum VTEnd {
    /// Emit this byte into the current DCS stream.
    Dcs,
    /// Emit this byte into the current OSC stream.
    Osc { used_bel: bool },
}

#[inline]
const fn is_c0(b: u8) -> bool {
    // Control characters, with the exception of the common whitespace controls.
    b <= 0x1F && b != b'\r' && b != b'\n' && b != b'\t'
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
const fn is_final(b: u8) -> bool {
    b >= 0x40 && b <= 0x7E
}
#[inline]
fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}
#[inline]
fn is_priv(b: u8) -> bool {
    matches!(b, b'<' | b'=' | b'>' | b'?')
}

macro_rules! byte_predicate {
    (|$p:ident| $body:block) => {{
        let mut out: [bool; 256] = [false; 256];
        let mut i = 0;
        while i < 256 {
            let $p: u8 = i as u8;
            out[i] = $body;
            i += 1;
        }
        out
    }};
}

const ENDS_CSI: [bool; 256] =
    byte_predicate!(|b| { is_final(b) || b == ESC || b == CAN || b == SUB });

const ENDS_GROUND: [bool; 256] = byte_predicate!(|b| { is_c0(b) || b == DEL });

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    EscInt,
    EscSs2,
    EscSs3,
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

/// No events from parser (ie, only emits [`VTEvent::Raw`] events)
pub const VT_PARSER_INTEREST_NONE: u8 = 0;
/// Request CSI events from parser.
pub const VT_PARSER_INTEREST_CSI: u8 = 1 << 0;
/// Request DCS events from parser.
pub const VT_PARSER_INTEREST_DCS: u8 = 1 << 1;
/// Request OSC events from parser.
pub const VT_PARSER_INTEREST_OSC: u8 = 1 << 2;
/// Request escape recovery events from parser.
pub const VT_PARSER_INTEREST_ESCAPE_RECOVERY: u8 = 1 << 4;
/// Request other events from parser.
pub const VT_PARSER_INTEREST_OTHER: u8 = 1 << 5;

/// Request all events from parser.
pub const VT_PARSER_INTEREST_ALL: u8 = VT_PARSER_INTEREST_CSI
    | VT_PARSER_INTEREST_DCS
    | VT_PARSER_INTEREST_OSC
    | VT_PARSER_INTEREST_ESCAPE_RECOVERY
    | VT_PARSER_INTEREST_OTHER;

/// Default interest level.
pub const VT_PARSER_INTEREST_DEFAULT: u8 = VT_PARSER_INTEREST_CSI
    | VT_PARSER_INTEREST_DCS
    | VT_PARSER_INTEREST_OSC
    | VT_PARSER_INTEREST_OTHER;

#[must_use]
trait MaybeAbortable {
    fn abort(self) -> bool;
}

impl MaybeAbortable for bool {
    #[inline(always)]
    fn abort(self) -> bool {
        !self
    }
}

impl MaybeAbortable for () {
    #[inline(always)]
    fn abort(self) -> bool {
        false
    }
}

/// A push parser for the VT/xterm protocol.
///
/// The parser can be configured to only emit certain types of events by setting
/// the `INTEREST` parameter.
pub struct VTPushParser<const INTEREST: u8 = VT_PARSER_INTEREST_DEFAULT> {
    st: State,

    // Header collectors for short escapes (we borrow from these in callbacks)
    ints: VTIntermediate,
    params: Params,
    cur_param: Param,
    priv_prefix: Option<u8>,
    held_byte: Option<u8>,
}

impl Default for VTPushParser {
    fn default() -> Self {
        Self::new()
    }
}

impl VTPushParser {
    pub const fn new() -> Self {
        VTPushParser::new_with()
    }

    /// Decode a buffer of bytes into a series of events.
    pub fn decode_buffer<'a>(input: &'a [u8], mut cb: impl for<'b> FnMut(VTEvent<'b>)) {
        let mut parser = VTPushParser::new();
        parser.feed_with(input, &mut cb);
        parser.finish(&mut cb);
    }

    pub const fn new_with_interest<const INTEREST: u8>() -> VTPushParser<INTEREST> {
        VTPushParser::new_with()
    }
}

/// Emit the EscInvalid event
macro_rules! invalid {
    ($self:ident .priv_prefix, $self_:ident .ints, $b:expr) => {
        if let Some(p) = $self.priv_prefix {
            if $self.ints.len() == 0 {
                VTEvent::EscInvalid(EscInvalid::Two(p, $b))
            } else if $self.ints.len() == 1 {
                VTEvent::EscInvalid(EscInvalid::Three(p, $self.ints.data[0], $b))
            } else {
                VTEvent::EscInvalid(EscInvalid::Four(
                    p,
                    $self.ints.data[0],
                    $self.ints.data[1],
                    $b,
                ))
            }
        } else {
            if $self.ints.len() == 0 {
                VTEvent::EscInvalid(EscInvalid::One($b))
            } else if $self.ints.len() == 1 {
                VTEvent::EscInvalid(EscInvalid::Two($self.ints.data[0], $b))
            } else {
                VTEvent::EscInvalid(EscInvalid::Three(
                    $self.ints.data[0],
                    $self.ints.data[1],
                    $b,
                ))
            }
        }
    };
    ($self:ident .priv_prefix, $self_:ident .ints) => {
        if let Some(p) = $self.priv_prefix {
            if $self.ints.len() == 0 {
                VTEvent::EscInvalid(EscInvalid::One(p))
            } else if $self.ints.len() == 1 {
                VTEvent::EscInvalid(EscInvalid::Two(p, $self.ints.data[0]))
            } else {
                VTEvent::EscInvalid(EscInvalid::Three(p, $self.ints.data[0], $self.ints.data[1]))
            }
        } else {
            if $self.ints.len() == 0 {
                // I don't think this can happen
                VTEvent::C0(0x1b)
            } else if $self.ints.len() == 1 {
                VTEvent::EscInvalid(EscInvalid::One($self.ints.data[0]))
            } else {
                VTEvent::EscInvalid(EscInvalid::Two($self.ints.data[0], $self.ints.data[1]))
            }
        }
    };
    ($a:expr) => {
        VTEvent::EscInvalid(EscInvalid::One($a))
    };
    ($a:expr, $b:expr) => {
        VTEvent::EscInvalid(EscInvalid::Two($a, $b))
    };
}

impl<const INTEREST: u8> VTPushParser<INTEREST> {
    const fn new_with() -> Self {
        Self {
            st: State::Ground,
            ints: VTIntermediate::empty(),
            params: SmallVec::new_const(),
            cur_param: SmallVec::new_const(),
            priv_prefix: None,
            held_byte: None,
        }
    }

    // =====================
    // Callback-driven API
    // =====================

    /// Feed bytes into the parser. This is the main entry point for the parser.
    /// It will call the callback with events as they are emitted.
    ///
    /// The callback must be valid for the lifetime of the `feed_with` call.
    ///
    /// The callback may emit any number of events (including zero), depending
    /// on the state of the internal parser.
    #[inline]
    pub fn feed_with<'this, 'input, F: for<'any> FnMut(VTEvent<'any>)>(
        &'this mut self,
        input: &'input [u8],
        cb: &mut F,
    ) {
        self.feed_with_internal(input, cb);
    }

    /// Feed bytes into the parser. This is the main entry point for the parser.
    /// It will call the callback with events as they are emitted.
    ///
    /// The callback must be valid for the lifetime of the `feed_with` call.
    /// Returning `true` will continue parsing, while returning `false` will
    /// stop.
    ///
    /// The callback may emit any number of events (including zero), depending
    /// on the state of the internal parser.
    ///
    /// This function returns the number of bytes processed. Note that some
    /// bytes may have been processed any not emitted.
    #[inline]
    pub fn feed_with_abortable<'this, 'input, F: for<'any> FnMut(VTEvent<'any>) -> bool>(
        &'this mut self,
        input: &'input [u8],
        cb: &mut F,
    ) -> usize {
        self.feed_with_internal(input, cb)
    }

    #[inline(always)]
    fn feed_with_internal<
        'this,
        'input,
        R: MaybeAbortable,
        F: for<'any> FnMut(VTEvent<'any>) -> R,
    >(
        &'this mut self,
        input: &'input [u8],
        cb: &mut F,
    ) -> usize {
        if input.is_empty() {
            return 0;
        }

        #[derive(Debug)]
        struct FeedState {
            buffer_idx: usize,
            current_emit: Option<VTEmit>,
            hold: bool,
        }

        let mut state = FeedState {
            buffer_idx: 0,
            current_emit: None,
            hold: self.held_byte.is_some(),
        };

        macro_rules! emit {
            ($state:ident, $i:expr, $cb:expr, $end:expr, $used_bel:expr) => {
                let hold = std::mem::take(&mut $state.hold);
                if let Some(emit) = $state.current_emit.take() {
                    let i = $i;
                    let range = $state.buffer_idx..(i - hold as usize);
                    if $end {
                        if match emit {
                            VTEmit::Ground => unreachable!(),
                            VTEmit::Dcs => $cb(VTEvent::DcsEnd(&input[range])),
                            VTEmit::Osc => $cb(VTEvent::OscEnd {
                                data: &input[range],
                                used_bel: $used_bel,
                            }),
                        }
                        .abort()
                        {
                            return i + 1;
                        }
                    } else if range.len() > 0 {
                        if match emit {
                            VTEmit::Ground => $cb(VTEvent::Raw(&input[range])),
                            VTEmit::Dcs => $cb(VTEvent::DcsData(&input[range])),
                            VTEmit::Osc => $cb(VTEvent::OscData(&input[range])),
                        }
                        .abort()
                        {
                            return i + 1;
                        }
                    }
                }
            };
        }

        let mut held_byte = self.held_byte.take();
        let mut i = 0;

        while i < input.len() {
            // Fast path for the common case of no ANSI escape sequences.
            if self.st == State::Ground {
                let start = i;
                loop {
                    if i >= input.len() {
                        cb(VTEvent::Raw(&input[start..]));
                        return input.len();
                    }
                    if ENDS_GROUND[input[i] as usize] {
                        break;
                    }
                    i += 1;
                }

                if start != i && cb(VTEvent::Raw(&input[start..i])).abort() {
                    return i;
                }

                if input[i] == ESC {
                    self.clear_hdr_collectors();
                    self.st = State::Escape;
                    i += 1;
                    continue;
                }
            }

            // Fast path: search for the CSI final
            if self.st == State::CsiIgnore {
                loop {
                    if i >= input.len() {
                        return input.len();
                    }
                    if ENDS_CSI[input[i] as usize] {
                        break;
                    }
                    i += 1;
                }

                if input[i] == ESC {
                    self.st = State::Escape;
                } else {
                    self.st = State::Ground;
                }
                i += 1;
                continue;
            }

            let action = self.push_with(input[i]);

            match action {
                VTAction::None => {
                    if let Some(emit) = state.current_emit {
                        // We received a DEL during an emit, so we need to partially emit our buffer
                        let range = state.buffer_idx..(i - state.hold as usize);
                        if !range.is_empty()
                            && match emit {
                                VTEmit::Ground => cb(VTEvent::Raw(&input[range])),
                                VTEmit::Dcs => cb(VTEvent::DcsData(&input[range])),
                                VTEmit::Osc => cb(VTEvent::OscData(&input[range])),
                            }
                            .abort()
                        {
                            if state.hold {
                                self.held_byte = Some(0x1b);
                            }
                            return i + 1;
                        }
                        if state.hold {
                            held_byte = Some(0x1b);
                        }
                        state.current_emit = None;
                    }
                }
                VTAction::Event(e) => {
                    if cb(e).abort() {
                        return i + 1;
                    }
                }
                VTAction::End(VTEnd::Dcs) => {
                    held_byte = None;
                    emit!(state, i, cb, true, false);
                }
                VTAction::End(VTEnd::Osc { used_bel }) => {
                    held_byte = None;
                    emit!(state, i, cb, true, used_bel);
                }
                VTAction::Buffer(emit) | VTAction::Hold(emit) => {
                    if state.current_emit.is_none() {
                        if let Some(h) = held_byte.take() {
                            if match emit {
                                VTEmit::Ground => cb(VTEvent::Raw(&[h])),
                                VTEmit::Dcs => cb(VTEvent::DcsData(&[h])),
                                VTEmit::Osc => cb(VTEvent::OscData(&[h])),
                            }
                            .abort()
                            {
                                if matches!(action, VTAction::Hold(_)) {
                                    self.held_byte = Some(0x1b);
                                    return 1;
                                }
                                return 0;
                            }
                        }
                    }

                    debug_assert!(state.current_emit.is_none() || state.current_emit == Some(emit));

                    state.hold = matches!(action, VTAction::Hold(_));
                    if state.current_emit.is_none() {
                        state.buffer_idx = i;
                        state.current_emit = Some(emit);
                    }
                }
                VTAction::Cancel(emit) => {
                    state.current_emit = None;
                    state.hold = false;
                    if match emit {
                        VTEmit::Ground => unreachable!(),
                        VTEmit::Dcs => cb(VTEvent::DcsCancel),
                        VTEmit::Osc => cb(VTEvent::OscCancel),
                    }
                    .abort()
                    {
                        return i + 1;
                    }
                }
            };
            i += 1;
        }

        // Is there more to emit?
        if state.hold {
            self.held_byte = Some(0x1b);
        }

        if let Some(emit) = state.current_emit.take() {
            let range = &input[state.buffer_idx..input.len() - state.hold as usize];
            if !range.is_empty() {
                match emit {
                    VTEmit::Ground => cb(VTEvent::Raw(range)),
                    VTEmit::Dcs => cb(VTEvent::DcsData(range)),
                    VTEmit::Osc => cb(VTEvent::OscData(range)),
                };
            }
        };

        // If we get this far, we processed the whole buffer
        input.len()
    }

    /// Returns true if the parser is in the ground state.
    pub fn is_ground(&self) -> bool {
        self.st == State::Ground
    }

    /// Feed an idle event into the parser. This will emit a C0(ESC) event if
    /// the parser is in the Escape state, and will silently cancel any EscInt
    /// state.
    pub fn idle(&mut self) -> Option<VTEvent<'static>> {
        match self.st {
            State::Escape => {
                self.st = State::Ground;
                Some(VTEvent::C0(ESC))
            }
            State::EscInt => {
                self.st = State::Ground;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    None
                } else {
                    Some(invalid!(self.priv_prefix, self.ints))
                }
            }
            State::EscSs2 | State::EscSs3 => {
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    self.st = State::Ground;
                    None
                } else {
                    let c = match self.st {
                        State::EscSs2 => SS2,
                        State::EscSs3 => SS3,
                        _ => unreachable!(),
                    };
                    self.st = State::Ground;
                    Some(invalid!(c))
                }
            }
            _ => None,
        }
    }

    fn push_with(&mut self, b: u8) -> VTAction {
        use State::*;
        match self.st {
            Ground => self.on_ground(b),
            Escape => self.on_escape(b),
            EscInt => self.on_esc_int(b),
            EscSs2 => self.on_esc_ss2(b),
            EscSs3 => self.on_esc_ss3(b),

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

    pub fn finish<F: FnMut(VTEvent)>(&mut self, _cb: &mut F) {
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
        VTAction::Event(VTEvent::Csi(CSI {
            private: privp,
            params: ParamBuf {
                params: &self.params,
            },
            intermediates: self.ints,
            final_byte,
        }))
    }

    fn dcs_start(&mut self, final_byte: u8) -> VTAction {
        self.finish_params_if_any();

        let privp = self.priv_prefix.take();
        VTAction::Event(VTEvent::DcsStart(DCS {
            private: privp,
            params: ParamBuf {
                params: &self.params,
            },
            intermediates: self.ints,
            final_byte,
        }))
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
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(b))
                }
            }
            // NOTE: DEL should be ignored normally, but for better recovery,
            // we move to ground state here instead.
            DEL => {
                self.st = Ground;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(b))
                }
            }
            c if is_intermediate(c) => {
                if self.ints.push(c) {
                    self.st = EscInt;
                } else {
                    self.st = Ground;
                }
                VTAction::None
            }
            c if is_priv(c) => {
                self.priv_prefix = Some(c);
                self.st = EscInt;
                VTAction::None
            }
            CSI => {
                if INTEREST & VT_PARSER_INTEREST_CSI == 0 {
                    self.st = CsiIgnore;
                } else {
                    self.st = CsiEntry;
                }
                VTAction::None
            }
            DCS => {
                if INTEREST & VT_PARSER_INTEREST_DCS == 0 {
                    self.st = DcsIgnore;
                } else {
                    self.st = DcsEntry;
                }
                VTAction::None
            }
            OSC => {
                self.st = OscString;
                VTAction::Event(VTEvent::OscStart)
            }
            SS2 => {
                self.st = EscSs2;
                VTAction::None
            }
            SS3 => {
                self.st = EscSs3;
                VTAction::None
            }
            SOS | PM | APC => {
                self.st = State::SosPmApcString;
                VTAction::None
            }
            c if is_final(c) || is_digit(c) => {
                self.st = Ground;
                VTAction::Event(VTEvent::Esc(Esc {
                    intermediates: self.ints,
                    private: self.priv_prefix.take(),
                    final_byte: c,
                }))
            }
            ESC => {
                // ESC ESC allowed, but we stay in the current state
                VTAction::Event(VTEvent::C0(ESC))
            }
            _ => {
                self.st = Ground;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(b))
                }
            }
        }
    }

    fn on_esc_int(&mut self, b: u8) -> VTAction {
        use State::*;
        match b {
            CAN | SUB => {
                self.st = Ground;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(self.priv_prefix, self.ints, b))
                }
            }
            // NOTE: DEL should be ignored normally, but for better recovery,
            // we move to ground state here instead.
            DEL => {
                self.st = Ground;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(self.priv_prefix, self.ints, b))
                }
            }
            c if is_intermediate(c) => {
                if !self.ints.push(c) {
                    self.st = Ground;
                    if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                        VTAction::None
                    } else {
                        VTAction::Event(invalid!(self.priv_prefix, self.ints, b))
                    }
                } else {
                    VTAction::None
                }
            }
            c if is_final(c) || is_digit(c) => {
                self.st = Ground;
                VTAction::Event(VTEvent::Esc(Esc {
                    intermediates: self.ints,
                    private: self.priv_prefix.take(),
                    final_byte: c,
                }))
            }
            // NOTE: We assume that we want to stay in the escape state
            // to recover from this state.
            ESC => {
                self.st = Escape;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(self.priv_prefix, self.ints))
                }
            }
            c => {
                self.st = Ground;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(self.priv_prefix, self.ints, c))
                }
            }
        }
    }

    fn on_esc_ss2(&mut self, b: u8) -> VTAction {
        use State::*;
        self.st = Ground;
        match b {
            CAN | SUB => {
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(SS2, b))
                }
            }
            // NOTE: We assume that we want to stay in the escape state
            // to recover from this state.
            ESC => {
                self.st = Escape;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(SS2))
                }
            }
            c => VTAction::Event(VTEvent::Ss2(SS2 { char: c })),
        }
    }

    fn on_esc_ss3(&mut self, b: u8) -> VTAction {
        use State::*;
        self.st = Ground;
        match b {
            CAN | SUB => {
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(SS3, b))
                }
            }
            // NOTE: We assume that we want to stay in the escape state
            // to recover from this state.
            ESC => {
                self.st = Escape;
                if INTEREST & VT_PARSER_INTEREST_ESCAPE_RECOVERY == 0 {
                    VTAction::None
                } else {
                    VTAction::Event(invalid!(SS3))
                }
            }
            c => VTAction::Event(VTEvent::Ss3(SS3 { char: c })),
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
                if self.ints.push(c) {
                    self.st = CsiInt;
                } else {
                    self.st = Ground;
                }
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
                if self.ints.push(c) {
                    self.st = CsiInt;
                } else {
                    self.st = Ground;
                }
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
                if self.ints.push(c) {
                    self.st = CsiInt;
                } else {
                    self.st = Ground;
                }
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
                if self.ints.push(c) {
                    self.st = DcsInt;
                } else {
                    self.st = Ground;
                }
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
                if self.ints.push(c) {
                    self.st = DcsInt;
                } else {
                    self.st = Ground;
                }
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
                if self.ints.push(c) {
                    self.st = DcsInt;
                } else {
                    self.st = Ground;
                }
                VTAction::None
            }
            c if is_final(c) || is_digit(c) || c == b':' || c == b';' => {
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
            ESC => VTAction::None,
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
                VTAction::End(VTEnd::Dcs)
            }
            DEL => VTAction::None,
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
                self.st = Ground;
                VTAction::End(VTEnd::Osc { used_bel: true })
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
                self.st = Ground;
                VTAction::End(VTEnd::Osc { used_bel: false })
            } // ST
            ESC => VTAction::Hold(VTEmit::Osc),
            DEL => VTAction::None,
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
            DEL => VTAction::None,
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
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_edge_cases() {
        // Test empty input
        let mut result = String::new();
        VTPushParser::decode_buffer(&[], |e| result.push_str(&format!("{e:?}\n")));
        assert_eq!(result.trim(), "");

        // Test single ESC
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1b", |e| result.push_str(&format!("{e:?}\n")));
        assert_eq!(result.trim(), "");

        // Test incomplete CSI
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1b[", |e| result.push_str(&format!("{e:?}\n")));
        assert_eq!(result.trim(), "");

        // Test incomplete DCS
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1bP", |e| result.push_str(&format!("{e:?}\n")));
        assert_eq!(result.trim(), "");

        // Test incomplete OSC
        let mut result = String::new();
        VTPushParser::decode_buffer(b"\x1b]", |e| result.push_str(&format!("{e:?}\n")));
        assert_eq!(result.trim(), "OscStart");
    }

    #[test]
    fn test_streaming_behavior() {
        // Test streaming DCS data
        let mut parser = VTPushParser::new(); // Small flush size
        let mut result = String::new();
        let mut callback = |vt_input: VTEvent<'_>| {
            result.push_str(&format!("{vt_input:?}\n"));
        };

        // Feed DCS data in chunks
        parser.feed_with(b"\x1bP1;2;3 |", &mut callback);
        parser.feed_with(b"data", &mut callback);
        parser.feed_with(b" more", &mut callback);
        parser.feed_with(b"\x1b\\", &mut callback);

        assert_eq!(
            result.trim(),
            "DcsStart(, '1', '2', '3', ' ', |)\nDcsData('data')\nDcsData(' more')\nDcsEnd('')"
        );
    }

    #[test]
    fn test_finish_method() {
        let mut parser = VTPushParser::new();
        let mut result = String::new();
        let mut callback = |vt_input: VTEvent<'_>| {
            result.push_str(&format!("{vt_input:?}\n"));
        };

        // Start an incomplete sequence
        parser.feed_with(b"\x1b[1;2;3", &mut callback);

        // Finish should flush any pending raw data
        parser.finish(&mut callback);

        assert_eq!(result.trim(), "");
    }

    // #[test]
    // fn test_dcs_payload_passthrough() {
    //     // Test cases for DCS payload passthrough behavior
    //     // Notes: body must be passed through verbatim.
    //     // - ESC '\' (ST) ends the string.
    //     // - ESC ESC stays as two bytes in the body.
    //     // - ESC X (X!='\') is data: both ESC and the following byte are payload.
    //     // - BEL (0x07) is data in DCS (not a terminator).

    //     let dcs_cases: &[(&[u8], &str)] = &[
    //         // 1) Minimal: embedded CSI SGR truecolor (colon params)
    //         (b"\x1bPq\x1b[38:2:12:34:56m\x1b\\", "<ESC>[38:2:12:34:56m"),
    //         // 2) Mixed payload: CSI + literal text
    //         (b"\x1bPq\x1b[48:2:0:0:0m;xyz\x1b\\", "<ESC>[48:2:0:0:0m;xyz"),
    //         // 3) DECRQSS-style reply payload (DCS 1$r ... ST) containing colon-CSI
    //         (
    //             b"\x1bP1$r\x1b[38:2:10:20:30;58:2::200:100:0m\x1b\\",
    //             "<ESC>[38:2:10:20:30;58:2::200:100:0m",
    //         ),
    //         // 4) ESC ESC and ESC X inside body (all data)
    //         (
    //             b"\x1bPqABC\x1b\x1bDEF\x1bXG\x1b\\",
    //             "ABC<ESC><ESC>DEF<ESC>XG",
    //         ),
    //         // 5) BEL in body (data, not a terminator)
    //         (b"\x1bPqDATA\x07MORE\x1b\\", "DATA<BEL>MORE"),
    //         // 6) iTerm2-style header (!|) with embedded CSI 256-color
    //         (b"\x1bP!|\x1b[38:5:208m\x1b\\", "<ESC>[38:5:208m"),
    //         // 7) Private prefix + final '|' (>|) with plain text payload
    //         (b"\x1bP>|Hello world\x1b\\", "Hello world"),
    //         // 8) Multiple embedded CSIs back-to-back
    //         (
    //             b"\x1bPq\x1b[38:2:1:2:3m\x1b[48:5:17m\x1b\\",
    //             "<ESC>[38:2:1:2:3m<ESC>[48:5:17m",
    //         ),
    //         // 9) Long colon param with leading zeros
    //         (
    //             b"\x1bPq\x1b[58:2::000:007:042m\x1b\\",
    //             "<ESC>[58:2::000:007:042m",
    //         ),
    //     ];

    //     for (input, expected_body) in dcs_cases {
    //         let events = collect_events(input);

    //         // Find DcsData events and concatenate their payloads
    //         let mut actual_body = String::new();
    //         for event in &events {
    //             if let Some(data_part) = event
    //                 .strip_prefix("DcsData('")
    //                 .and_then(|s| s.strip_suffix("')"))
    //             {
    //                 actual_body
    //                     .push_str(&data_part.replace("\x1b", "<ESC>").replace("\x07", "<BEL>"));
    //             }
    //         }

    //         assert_eq!(
    //             actual_body, *expected_body,
    //             "DCS payload mismatch for input {:?}. Full events: {:#?}",
    //             input, events
    //         );

    //         // Also verify we get proper DcsStart and DcsEnd events
    //         assert!(
    //             events.iter().any(|e| e.starts_with("DcsStart")),
    //             "Missing DcsStart for input {:?}. Events: {:#?}",
    //             input,
    //             events
    //         );
    //         assert!(
    //             events.iter().any(|e| e == "DcsEnd"),
    //             "Missing DcsEnd for input {:?}. Events: {:#?}",
    //             input,
    //             events
    //         );
    //     }
    // }

    fn collect_events(input: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let mut p = VTPushParser::new();
        p.feed_with(input, &mut |ev| out.push(format!("{ev:?}")));
        out
    }

    #[test]
    fn dcs_esc_esc_del() {
        // ESC P 1:2 q ... ST   -> colon inside header params (invalid)
        let ev = collect_events(b"\x1bP1;2;3|\x1b\x1b\x7fdata\x1b\\");
        // Expect: no DcsStart; the whole thing is ignored until ST
        eprintln!("{ev:?}");
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
        p.feed_with(input, &mut |ev| out.push(format!("{ev:?}")));
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

    /// Brute force sweep of all three-byte sequences to ensure we can recover
    /// from all invalid escape sequences (unless CSI/OSC/DCS/SOS/PM/APC).
    #[test]
    fn three_byte_sequences_capturable() {
        let mut bytes = vec![];
        for i in 0..=0xFFFFFF_u32 {
            bytes.clear();
            let test_bytes = i.to_le_bytes();
            let test_bytes = &test_bytes[..3];
            if test_bytes.iter().any(|b| b == &0) {
                continue;
            }
            if test_bytes[0] == 0x1b && matches!(test_bytes[1], CSI | DCS | OSC | APC | PM | SOS) {
                continue;
            }
            if test_bytes[1] == 0x1b && matches!(test_bytes[2], CSI | DCS | OSC | APC | PM | SOS) {
                continue;
            }

            let mut parser = VTPushParser::<VT_PARSER_INTEREST_ALL>::new_with();
            parser.feed_with(test_bytes, &mut |event| {
                let mut chunk = [0_u8; 3];
                let b = event.encode(&mut chunk).unwrap_or_else(|_| {
                    panic!("Failed to encode event {test_bytes:X?} -> {event:?}")
                });
                bytes.extend_from_slice(&chunk[..b]);
            });
            if let Some(event) = parser.idle() {
                let mut chunk = [0_u8; 3];
                let b = event.encode(&mut chunk).unwrap_or_else(|_| {
                    panic!("Failed to encode event {test_bytes:X?} -> {event:?}")
                });
                bytes.extend_from_slice(&chunk[..b]);
            }

            if bytes.len() != 3 || bytes != test_bytes {
                eprintln!("Failed to parse:");
                parser.feed_with(test_bytes, &mut |event| {
                    eprintln!("{event:?}");
                });
                if let Some(event) = parser.idle() {
                    eprintln!("{event:?}");
                }
                assert_eq!(bytes, test_bytes, "{test_bytes:X?} -> {bytes:X?}");
            }
        }
    }
}
