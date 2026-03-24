use vt_push_parser::event::{CSI, SS2, SS3, VTEvent};
use vt_push_parser::{VTPushParser, capture};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteEvent {
    Start,
    End,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    Extra(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    Press(MouseButton),
    Release(MouseButton),
    Drag(MouseButton),
    Motion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub x: u16,
    pub y: u16,
    pub modifiers: Modifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent<'a> {
    /// A named key on the keyboard
    Key(keys::Key, Modifier),
    /// An unnamed key representable by a single character, but with modifiers
    /// that are more than just "shift".
    KeyChar(char, Modifier),
    /// A single text-producing character.
    Char(char),
    /// A mouse event.
    Mouse(MouseEvent),
    /// A report from the terminal.
    // Report(InputReport),
    /// A bracketed paste event.
    Paste(PasteEvent, &'a [u8]),
    /// A raw VT CSI event.
    Csi(CSI<'a>),
    /// A raw VT OSC event.
    Osc(&'a [u8]),
    /// A raw VT DCS event.
    Dcs(&'a [u8]),
    /// A raw VT SS2 event.
    Ss2(SS2),
    /// A raw VT SS3 event.
    Ss3(SS3),
}

/// Common input reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputReport {
    // CSI Ps1 ; Ps2 R
    CursorPosition(u16, u16),
    // CSI Pl; Pc; Pp R (DECXCPR)
    CursorPositionPage(u16, u16, u16),
    // CSI Pn ; n or CSI ? Pn ; n
    DeviceStatus(bool, u16),
    // CSI ? Pn1 ; Pn2 ; n or CSI ? Pn1 ; Pn2 ; n
    DeviceStatus2(bool, u16, u16),
    // CSI Ps1; ... Psn (if Ps! is 61-64, identifies VT level, others are bitflags)
    DeviceAttributes1(u8, u32),
    // CSI > Pp ; Pv ; Pc c
    DeviceAttributes2(u16, u16, u8),
    // CSI Ps1 ; Ps2 $ y or CSI ? Ps1 ; Ps2 $ y
    ReportMode(bool, u16, u8),
    // CSI 1 t or CSI 2 t
    ReportWindowState(bool),
    // CSI 3 ; x ; y t
    ReportWindowPosition(u16, u16),
    // CSI 4 ; h ; w t
    ReportWindowSizePixel(u16, u16),
    // CSI 5 ; h ; w t
    ReportTextSizeCharacter(u16, u16),
    // CSI 6 ; h ; w t
    ReportScreenSizeCharacter(u16, u16),
    // OSC Ps1 ; rgb:... BEL
    ReportDynamicColor(u8, u16, u16, u16),
    // ESC I
    FocusIn,
    // ESC O
    FocusOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Modifier(pub(crate) u8);

bitflags::bitflags! {
    impl Modifier: u8 {
        const SHIFT = 1 << 0;
        const ALT = 1 << 1;
        const CTRL = 1 << 2;

        const KEYPAD = 1 << 3;
        const SUN = 1 << 4;
        const VT52 = 1 << 5;
    }
}

mod keys {
    use super::Modifier;

    macro_rules! e {
        ($depth:literal, $c:literal, ($($mod:ident),*)) => {
            MatchResult::Match {
                length: $depth,
                what: pack_mod($c as _, (Modifier::empty() $( | Modifier::$mod )*).0),
            }
        };
        ($depth:literal, $name:ident, ($($mod:ident),*)) => {
            MatchResult::Match {
                length: $depth,
                what: pack_mod(Key::$name as _, 0x80 | (Modifier::empty() $( | Modifier::$mod )*).0),
            }
        };
    }

    // Byte-packed to 8 bytes
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum MatchResult {
        /// We didn't find a match and we know that at least this many bytes
        /// won't match.
        NoMatch {
            length: u8,
        },
        PendingMatch,
        Match {
            length: u8,
            what: u32,
        },
    }

    #[inline(always)]
    const fn pack(char: char, alt: bool) -> u32 {
        char as u32 | (((if alt { Modifier::ALT.0 as _ } else { 0 }) as u32) << 24)
    }

    #[inline(always)]
    const fn pack_mod(char: u32, modifiers: u8) -> u32 {
        char | (modifiers as u32) << 24
    }

    #[inline(always)]
    fn utf8_1(alt: bool, byte: u8) -> MatchResult {
        MatchResult::Match {
            length: 1 + alt as u8,
            what: pack(byte as char, alt),
        }
    }

    #[inline(always)]
    fn utf8_2(alt: bool, bytes: &[u8]) -> MatchResult {
        if bytes.len() < 2 {
            return MatchResult::PendingMatch;
        }
        // 110xxxxx 10xxxxxx
        let Some(char) =
            char::from_u32((bytes[0] & 0b11111) as u32 | ((bytes[1] & 0b111111) as u32) << 6)
        else {
            return MatchResult::Match {
                length: 2 + alt as u8,
                what: pack('\u{FFFD}', false),
            };
        };
        MatchResult::Match {
            length: 2 + alt as u8,
            what: pack(char, alt),
        }
    }

    #[inline(always)]
    fn utf8_3(alt: bool, bytes: &[u8]) -> MatchResult {
        if bytes.len() < 3 {
            return MatchResult::PendingMatch;
        }
        // 1110xxxx 10xxxxxx 10xxxxxx
        let Some(char) = char::from_u32(
            (((bytes[0] & 0b1111) as u32) << 12)
                | (((bytes[1] & 0b111111) as u32) << 6)
                | ((bytes[2] & 0b111111) as u32),
        ) else {
            return MatchResult::Match {
                length: 3 + alt as u8,
                what: pack('\u{FFFD}', false),
            };
        };
        MatchResult::Match {
            length: 3 + alt as u8,
            what: pack(char, alt),
        }
    }

    #[inline(always)]
    fn utf8_4(alt: bool, bytes: &[u8]) -> MatchResult {
        if bytes.len() < 4 {
            return MatchResult::PendingMatch;
        }
        // 11110xxx 10xxxxxx 10xxxxxx 10xxxxxx
        let Some(char) = char::from_u32(
            (((bytes[0] & 0b111) as u32) << 18)
                | (((bytes[1] & 0b111111) as u32) << 12)
                | (((bytes[2] & 0b111111) as u32) << 6)
                | ((bytes[3] & 0b111111) as u32),
        ) else {
            return MatchResult::Match {
                length: 4 + alt as u8,
                what: pack('\u{FFFD}', false),
            };
        };
        MatchResult::Match {
            length: 4 + alt as u8,
            what: pack(char, alt),
        }
    }

    #[inline(always)]
    fn invalid_utf8(_: u8) -> MatchResult {
        MatchResult::Match {
            length: 1,
            what: pack('\u{FFFD}', false),
        }
    }

    pub fn find_sequence(bytes: &[u8]) -> MatchResult {
        match find_sequence_nonidle_gen(bytes) {
            MatchResult::NoMatch { length } => {
                if matches!(bytes, [0x1b, 0x1b, ..]) {
                    MatchResult::Match {
                        length: 1,
                        what: pack('\x1b', false),
                    }
                } else {
                    MatchResult::NoMatch { length }
                }
            }
            res => res,
        }
    }

    #[allow(unused)]
    pub fn find_sequence_idle(bytes: &[u8]) -> MatchResult {
        match find_sequence_idle_gen(bytes) {
            MatchResult::NoMatch { length } => match bytes {
                [0x1b] => MatchResult::Match {
                    length: 1,
                    what: pack('\x1b', false),
                },
                [0x1b, 0x1b] => MatchResult::Match {
                    length: 2,
                    what: pack('\x1b', true),
                },
                [0x1b, 0x1b, ..] => MatchResult::Match {
                    length: 1,
                    what: pack('\x1b', false),
                },
                _ => MatchResult::NoMatch { length },
            },
            res => res,
        }
    }

    include!(concat!(env!("OUT_DIR"), "/keys.rs"));
}

#[derive(Debug, Default)]
enum CaptureState {
    #[default]
    None,
    Paste,
    Mouse,
}

/// Decodes the mouse button byte used by X10, SGR, and urxvt protocols.
///
/// Returns (button, modifiers, is_motion).
fn decode_mouse_button_byte(cb: u16) -> (MouseButton, Modifier, bool) {
    let modifiers = Modifier::from_bits_truncate(
        (if cb & 0x04 != 0 { Modifier::SHIFT.0 } else { 0 })
            | (if cb & 0x08 != 0 { Modifier::ALT.0 } else { 0 })
            | (if cb & 0x10 != 0 { Modifier::CTRL.0 } else { 0 }),
    );
    let is_motion = cb & 0x20 != 0;
    let button_bits = cb & 0xC3; // bits 0-1 and 6-7
    let button = match button_bits {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        64 => MouseButton::WheelUp,
        65 => MouseButton::WheelDown,
        66 => MouseButton::WheelLeft,
        67 => MouseButton::WheelRight,
        n => MouseButton::Extra(n as u8),
    };
    (button, modifiers, is_motion)
}

enum HandleAction<'a> {
    Handled,
    Capture(CaptureState, capture::VTCaptureInternal),
    Unhandled(VTEvent<'a>),
}

/// A push parser for the VT/xterm input protocol.
///
/// This parser enables parsing of key sequences alongside the normal VT/xterm
/// protocol. In addition, most common input control sequences are automatically
/// decoded.
///
/// Unrecognized sequences are emitted as raw [`VTEvent`]s.
pub struct VTPushParserInput {
    key_buffer: [u8; keys::MAX_SEQUENCE_LEN],
    key_buffer_len: usize,
    data_accumulator: Vec<u8>,

    capture: capture::VTCaptureInternal,
    capture_state: CaptureState,
    parser: VTPushParser,
}

fn handle_vt_event<'a>(event: VTEvent<'a>, cb: &mut impl FnMut(InputEvent)) -> HandleAction<'a> {
    if let VTEvent::Csi(ref csi) = event {
        match (csi.private, csi.final_byte) {
            // SGR mouse press/drag/motion: CSI < Pb ; Px ; Py M
            (Some(b'<'), b'M') => {
                if let (Some(pb), Some(px), Some(py)) = (
                    csi.params.try_parse::<u16>(0),
                    csi.params.try_parse::<u16>(1),
                    csi.params.try_parse::<u16>(2),
                ) {
                    let (button, modifiers, is_motion) = decode_mouse_button_byte(pb);
                    let kind = if is_motion {
                        MouseEventKind::Drag(button)
                    } else {
                        MouseEventKind::Press(button)
                    };
                    cb(InputEvent::Mouse(MouseEvent {
                        kind,
                        x: px.saturating_sub(1),
                        y: py.saturating_sub(1),
                        modifiers,
                    }));
                    return HandleAction::Handled;
                }
            }
            // SGR mouse release: CSI < Pb ; Px ; Py m
            (Some(b'<'), b'm') => {
                if let (Some(pb), Some(px), Some(py)) = (
                    csi.params.try_parse::<u16>(0),
                    csi.params.try_parse::<u16>(1),
                    csi.params.try_parse::<u16>(2),
                ) {
                    let (button, modifiers, _) = decode_mouse_button_byte(pb);
                    cb(InputEvent::Mouse(MouseEvent {
                        kind: MouseEventKind::Release(button),
                        x: px.saturating_sub(1),
                        y: py.saturating_sub(1),
                        modifiers,
                    }));
                    return HandleAction::Handled;
                }
            }
            // X10 or urxvt mouse: CSI [Pb ; Px ; Py] M
            (None, b'M') => {
                if csi.params.is_empty() {
                    // X10 mouse: CSI M followed by 3 raw bytes
                    return HandleAction::Capture(
                        CaptureState::Mouse,
                        capture::VTCaptureInternal::Count(3),
                    );
                } else if let (Some(pb), Some(px), Some(py)) = (
                    csi.params.try_parse::<u16>(0),
                    csi.params.try_parse::<u16>(1),
                    csi.params.try_parse::<u16>(2),
                ) {
                    // urxvt mouse: CSI Pb ; Px ; Py M
                    let (button, modifiers, is_motion) = decode_mouse_button_byte(pb);
                    let kind = if is_motion {
                        MouseEventKind::Drag(button)
                    } else {
                        MouseEventKind::Press(button)
                    };
                    cb(InputEvent::Mouse(MouseEvent {
                        kind,
                        x: px.saturating_sub(1),
                        y: py.saturating_sub(1),
                        modifiers,
                    }));
                    return HandleAction::Handled;
                }
            }
            (None, b'u') => match csi.params.len() {
                1 => {}
                2 => {}
                _ => {}
            },
            (None | Some(b'>'), b'~') => match csi.params.len() {
                2 => {}
                3 => {}
                _ => {}
            },
            _ => {}
        }
    }

    HandleAction::Unhandled(event)
}

fn handle_key_event(what: u32, mut cb: impl FnMut(InputEvent)) {
    if what & 0x80000000 != 0 {
        let key = keys::Key::try_from(what as u8).unwrap();
        cb(InputEvent::Key(key, Modifier(((what >> 24) & !0x80) as u8)));
    } else {
        cb(InputEvent::KeyChar(
            char::from_u32(what & 0xffffff).unwrap(),
            Modifier((what >> 24) as u8),
        ));
    }
}

impl Default for VTPushParserInput {
    fn default() -> Self {
        Self::new()
    }
}

impl VTPushParserInput {
    pub fn new() -> Self {
        Self {
            key_buffer: [0; keys::MAX_SEQUENCE_LEN],
            key_buffer_len: 0,
            capture: capture::VTCaptureInternal::None,
            capture_state: CaptureState::None,
            parser: VTPushParser::new(),
            data_accumulator: Vec::with_capacity(256),
        }
    }

    pub fn feed_with(&mut self, mut bytes: &[u8], mut cb: impl FnMut(InputEvent)) {
        // Handle the result of handle_vt_event, setting up capture or emitting
        // unhandled events as raw CSI.
        macro_rules! dispatch_vt {
            ($action:expr, $cb:expr, $capture:expr, $capture_state:expr) => {
                match $action {
                    HandleAction::Handled => {}
                    HandleAction::Capture(state, internal) => {
                        *$capture = internal;
                        *$capture_state = state;
                    }
                    HandleAction::Unhandled(event) => {
                        let input_event = match event {
                            VTEvent::Csi(csi) => Some(InputEvent::Csi(csi)),
                            VTEvent::OscEnd { data, .. } => Some(InputEvent::Osc(data)),
                            VTEvent::DcsEnd(data) => Some(InputEvent::Dcs(data)),
                            VTEvent::Ss2(ss2) => Some(InputEvent::Ss2(ss2)),
                            VTEvent::Ss3(ss3) => Some(InputEvent::Ss3(ss3)),
                            _ => None,
                        };
                        if let Some(e) = input_event {
                            $cb(e);
                        }
                    }
                }
            };
        }

        loop {
            // First, check if we have an active capture.
            if let Some(captured) = self.capture.feed(&mut bytes) {
                self.data_accumulator.extend_from_slice(captured);
                match std::mem::take(&mut self.capture_state) {
                    CaptureState::None => {}
                    CaptureState::Paste => {
                        cb(InputEvent::Paste(PasteEvent::End, &self.data_accumulator));
                        self.data_accumulator.clear();
                    }
                    CaptureState::Mouse => {
                        let data = &self.data_accumulator;
                        if data.len() >= 3 {
                            // X10 protocol: all three bytes are offset by 32
                            let cb_byte = (data[0] as u16).saturating_sub(32);
                            let x = (data[1] as u16).saturating_sub(33);
                            let y = (data[2] as u16).saturating_sub(33);
                            // In X10 protocol, button byte 3 (bits 0-1) means release
                            let is_release = cb_byte & 0x03 == 3;
                            let (button, modifiers, is_motion) =
                                decode_mouse_button_byte(cb_byte);
                            let kind = if is_release {
                                // X10 doesn't tell us which button was released
                                MouseEventKind::Release(MouseButton::Left)
                            } else if is_motion {
                                MouseEventKind::Drag(button)
                            } else {
                                MouseEventKind::Press(button)
                            };
                            cb(InputEvent::Mouse(MouseEvent {
                                kind,
                                x,
                                y,
                                modifiers,
                            }));
                        }
                        self.data_accumulator.clear();
                    }
                }
                continue;
            }

            // If no active capture, feed the parser if it's not in ground state, otherwise the key buffer.
            if !self.parser.is_ground() {
                let capture = &mut self.capture;
                let capture_state = &mut self.capture_state;
                let read = self.parser.feed_with_abortable(bytes, |event: VTEvent| {
                    let action = handle_vt_event(event, &mut cb);
                    dispatch_vt!(action, cb, capture, capture_state);
                    false
                });
                bytes = &bytes[read..];
                continue;
            }

            // If the key buffer has some bytes, feed more to it
            if self.key_buffer_len > 0 {
                let to_copy_into = &mut self.key_buffer[self.key_buffer_len
                    ..keys::MAX_SEQUENCE_LEN.min(self.key_buffer_len + bytes.len())];
                to_copy_into.copy_from_slice(&bytes[..to_copy_into.len()]);
                self.key_buffer_len += to_copy_into.len();
                bytes = &bytes[to_copy_into.len()..];

                match keys::find_sequence(&self.key_buffer[..self.key_buffer_len]) {
                    keys::MatchResult::Match { length, what } => {
                        self.key_buffer.rotate_left(length as _);
                        self.key_buffer_len -= length as usize;
                        handle_key_event(what, &mut cb);
                    }
                    keys::MatchResult::NoMatch { length } => {
                        // We don't have a match and we know that at least this many bytes
                        // won't match.
                        self.key_buffer.rotate_left(length as _);
                        let capture = &mut self.capture;
                        let capture_state = &mut self.capture_state;
                        // Feed them unconditionally to the parser
                        self.parser.feed_with_abortable(
                            &self.key_buffer[keys::MAX_SEQUENCE_LEN - length as usize..],
                            |event: VTEvent<'_>| {
                                let action = handle_vt_event(event, &mut cb);
                                dispatch_vt!(action, cb, capture, capture_state);
                                false
                            },
                        );
                    }
                    keys::MatchResult::PendingMatch => {
                        // We need more bytes to complete the match, which means we need another
                        // feed call.
                        return;
                    }
                }
            }

            // If we get this far, we're in the ground state for everything...

            if self.key_buffer_len == 0 {
                match keys::find_sequence(bytes) {
                    keys::MatchResult::Match { length, what } => {
                        bytes = &bytes[length as usize..];
                        handle_key_event(what, &mut cb);
                    }
                    keys::MatchResult::NoMatch { length } => {
                        // We don't have a match and we know that at least this many bytes
                        // won't match.
                        let capture = &mut self.capture;
                        let capture_state = &mut self.capture_state;
                        self.parser
                            .feed_with_abortable(&bytes[..length as usize], |event: VTEvent<'_>| {
                                let action = handle_vt_event(event, &mut cb);
                                dispatch_vt!(action, cb, capture, capture_state);
                                false
                            });
                        bytes = &bytes[length as usize..];
                    }
                    keys::MatchResult::PendingMatch => {
                        // We need more bytes to complete the match, which means we need another
                        // feed call.
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codegen() {
        use keys::{find_sequence, find_sequence_idle};

        macro_rules! m {
            ($bytes:literal, $length:literal) => {
                let seq = find_sequence($bytes);
                assert!(
                    matches!(
                        seq,
                        keys::MatchResult::Match {
                            length: $length,
                            what: _
                        }
                    ),
                    "{:?} => {seq:?}, expected match when not idle",
                    $bytes
                );
                let seq = find_sequence_idle($bytes);
                assert!(
                    matches!(
                        seq,
                        keys::MatchResult::Match {
                            length: $length,
                            what: _
                        }
                    ),
                    "{:?} => {seq:?}, expected match when idle",
                    $bytes
                );
            };
            ($bytes:literal, no) => {
                let seq = find_sequence($bytes);
                assert!(
                    matches!(seq, keys::MatchResult::NoMatch { length: _ }),
                    "{:?} => {seq:?}, expected no match",
                    $bytes
                );
            };
            ($bytes:literal, no, $length:literal) => {
                let seq = find_sequence($bytes);
                assert!(
                    matches!(seq, keys::MatchResult::PendingMatch),
                    "{:?} => {seq:?}, expected pending match when not idle",
                    $bytes
                );
                let seq = find_sequence_idle($bytes);
                assert!(
                    matches!(
                        seq,
                        keys::MatchResult::Match {
                            length: $length,
                            what: _
                        }
                    ),
                    "{:?} => {seq:?}, expected match when idle",
                    $bytes
                );
            };
        }

        // Unambiguous, same matches regardless of more data
        m!(b"\x1b[A", 3);
        m!(b"\x1bA", 2);
        m!(b"\x1bOA", 3);
        m!(b"\x1b\x01", 2);
        m!(b"\x1b\x1b[Z", 4);
        m!(b"\x1b1", 2); // ESC + 1

        // Ambiguous
        m!(b"\x1b", no, 1); // could be ESC or ESC ...
        m!(b"\x1b\x1b", no, 2); // could be ESC ESC or ESC CSI Z, for example
        m!(b"\x1bO", no, 2); // could be ESC O or ESC O A (SS3)
        m!(b"\x1b?", no, 2); // could be ESC ? or ESC ? . (VT52 keypad)

        // Super ambiguous escape chains
        m!(b"\x1b\x1b\x1b", 1); // could be a series of escapes, alt escapes, or whatever. yield one escape to be safe
        m!(b"\x1b\x1b\x1b\x1b", 1); // could be a series of escapes, alt escapes, or whatever. yield one escape to be safe

        // Shorter matches w/more data
        m!(b"\x1b!\x1b", 2); // ESC + ! (ALT + SHIFT + 1)
        m!(b"\x1b\x1b1", 1); // ESC
        m!(b"\x1b1\x1b", 2); // ESC + 1
        m!(b"\x1b1m", 2); // ESC + 1

        // Weak matches
        m!(b"\x1b\x1b[I", 1); // ESC + FocusIn (ESC is a weak match)
        m!(b"\x1b\x1bON", 1); // ESC + SS3 (ESC)
        m!(b"\x1b[", no, 2); // ESC + [ if idle, otherwise wait for more
        m!(b"\x1b\x1b[", no, 1); // ESC if idle, otherwise wait for more

        // No match
        m!(b"\x1b[I", no); // valid CSI
        m!(b"\x1b=", no);
        m!(b"\x1b<", no);
        m!(b"\x1b>", no);

        // Maybe re-visit
        m!(b"\x1bO\x1b", no); // disambiguation must end a string, but we might be able to do a lookahead here
    }

    #[test]
    fn test_find_sequence() {
        let bytes = b"\x1ba\x1b[Aa\xf0\x9f\x9b\x9c\x1b[B".as_slice();
        let mut input_parser = VTPushParserInput::new();
        let mut events = vec![];
        input_parser.feed_with(bytes, |event| {
            events.push(format!("{event:?}"));
        });
        assert!(!events.is_empty());
    }

    fn collect_events(bytes: &[u8]) -> Vec<InputEvent<'static>> {
        let mut events = vec![];
        let mut input_parser = VTPushParserInput::new();
        input_parser.feed_with(bytes, |event| {
            // Safety: we only inspect the event, and Mouse/Key events don't borrow
            let event: InputEvent<'static> = unsafe { std::mem::transmute(event) };
            events.push(event);
        });
        events
    }

    // SGR mouse tests

    #[test]
    fn test_sgr_mouse_left_click() {
        // CSI < 0 ; 10 ; 20 M  (left button press at 10,20)
        let events = collect_events(b"\x1b[<0;10;20M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_sgr_mouse_right_click() {
        // CSI < 2 ; 5 ; 3 M
        let events = collect_events(b"\x1b[<2;5;3M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Right),
                x: 4,
                y: 2,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_sgr_mouse_middle_click() {
        // CSI < 1 ; 1 ; 1 M
        let events = collect_events(b"\x1b[<1;1;1M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Middle),
                x: 0,
                y: 0,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_sgr_mouse_release() {
        // CSI < 0 ; 10 ; 20 m  (left button release)
        let events = collect_events(b"\x1b[<0;10;20m");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Release(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_sgr_mouse_wheel_up() {
        // CSI < 64 ; 10 ; 20 M
        let events = collect_events(b"\x1b[<64;10;20M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::WheelUp),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_sgr_mouse_wheel_down() {
        // CSI < 65 ; 10 ; 20 M
        let events = collect_events(b"\x1b[<65;10;20M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::WheelDown),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_sgr_mouse_drag() {
        // CSI < 32 ; 15 ; 25 M  (left button drag, bit 5 = motion)
        let events = collect_events(b"\x1b[<32;15;25M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                x: 14,
                y: 24,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_sgr_mouse_shift_click() {
        // CSI < 4 ; 10 ; 20 M  (shift + left click, bit 2 = shift)
        let events = collect_events(b"\x1b[<4;10;20M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::SHIFT,
            })
        );
    }

    #[test]
    fn test_sgr_mouse_ctrl_click() {
        // CSI < 16 ; 10 ; 20 M  (ctrl + left click, bit 4 = ctrl)
        let events = collect_events(b"\x1b[<16;10;20M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::CTRL,
            })
        );
    }

    #[test]
    fn test_sgr_mouse_alt_click() {
        // CSI < 8 ; 10 ; 20 M  (alt + left click, bit 3 = alt)
        let events = collect_events(b"\x1b[<8;10;20M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::ALT,
            })
        );
    }

    #[test]
    fn test_sgr_mouse_large_coordinates() {
        // CSI < 0 ; 500 ; 300 M
        let events = collect_events(b"\x1b[<0;500;300M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 499,
                y: 299,
                modifiers: Modifier::empty(),
            })
        );
    }

    // urxvt mouse tests

    #[test]
    fn test_urxvt_mouse_left_click() {
        // CSI 0 ; 10 ; 20 M  (urxvt: no '<' private prefix)
        let events = collect_events(b"\x1b[0;10;20M");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
    }

    // X10 mouse tests

    #[test]
    fn test_x10_mouse_left_click() {
        // CSI M followed by 3 bytes: button=0+32=0x20, x=10+32=0x2a, y=20+32=0x34
        let events = collect_events(b"\x1b[M\x20\x2a\x34");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_x10_mouse_right_click() {
        // button=2+32=0x22, x=5+32=0x25, y=3+32=0x23
        let events = collect_events(b"\x1b[M\x22\x25\x23");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Right),
                x: 4,
                y: 2,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_x10_mouse_release() {
        // button=3+32=0x23 means release, x=10+32=0x2a, y=20+32=0x34
        let events = collect_events(b"\x1b[M\x23\x2a\x34");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Release(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
    }

    #[test]
    fn test_x10_mouse_ctrl_click() {
        // button=0+16(ctrl)+32=0x30, x=10+32=0x2a, y=20+32=0x34
        let events = collect_events(b"\x1b[M\x30\x2a\x34");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::CTRL,
            })
        );
    }

    #[test]
    fn test_sgr_mouse_followed_by_key() {
        // Mouse event followed by a key press
        let events = collect_events(b"\x1b[<0;10;20Ma");
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 9,
                y: 19,
                modifiers: Modifier::empty(),
            })
        );
        assert_eq!(events[1], InputEvent::KeyChar('a', Modifier::empty()));
    }
}
