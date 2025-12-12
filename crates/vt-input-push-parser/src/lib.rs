use vt_push_parser::event::{CSI, SS2, SS3, VTEvent};
use vt_push_parser::{VTPushParser, capture};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteEvent {
    Start,
    End,
    Continue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEvent {
    Button(u8, Modifier),
    ButtonRelease(u8, Modifier),
    Motion(u16, u16),
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
    MouseHilite,
    MouseDrag,
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

fn handle_vt_event(event: VTEvent<'_>, mut cb: impl FnMut(InputEvent)) {
    println!("VTEvent: {event:?}");
    if let VTEvent::Csi(csi) = &event { match (csi.private, csi.final_byte) {
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
    } }

    // // Xterm standard mouse events (three utf-8 or three bytes after)
    // _EV_MOUSE: CSI M
    // // Mouse highlight mode. If the start and end coordinates are the same locations
    // // two bytes of data follow (x, y)
    // _EV_MOUSE_HILITE_CLICK: CSI t
    // // ... otherwise six bytes of data follow (startx, starty, endx, endy, mousex, and mousey)
    // _EV_MOUSE_HILITE_DRAG: CSI T

    cb(InputEvent::Csi(event.csi().unwrap()));
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
                        let s = str::from_utf8(&self.data_accumulator);
                        self.data_accumulator.clear();
                        cb(InputEvent::Mouse(MouseEvent::Button(
                            captured[0],
                            Modifier(captured[1]),
                        )));
                    }
                    CaptureState::MouseHilite => {
                        cb(InputEvent::Mouse(MouseEvent::Button(
                            captured[0],
                            Modifier(captured[1]),
                        )));
                    }
                    CaptureState::MouseDrag => {
                        cb(InputEvent::Mouse(MouseEvent::Button(
                            captured[0],
                            Modifier(captured[1]),
                        )));
                    }
                }
                continue;
            }

            // If no active capture, feed the parser if it's not in ground state, otherwise the key buffer.
            if !self.parser.is_ground() {
                let read = self.parser.feed_with_abortable(bytes, |event: VTEvent| {
                    handle_vt_event(event, &mut cb);
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
                        // Feed them unconditionally to the parser
                        self.parser.feed_with(
                            &self.key_buffer[keys::MAX_SEQUENCE_LEN - length as usize..],
                            |event: VTEvent<'_>| {
                                handle_vt_event(event, &mut cb);
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
                        self.parser
                            .feed_with(&bytes[..length as usize], |event: VTEvent<'_>| {
                                handle_vt_event(event, &mut cb);
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
        input_parser.feed_with(bytes, |event| {
            println!("Event: {event:?}");
            // assert_eq!(event, InputEvent::Key(keys::KeyCode::UP, Modifier::empty()));
        });
    }
}
