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
