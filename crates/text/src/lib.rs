//! The interface's words: one typed table behind a language switch.
//!
//! Every user-facing string the editor draws is a [`Message`], and every language it can draw is a
//! [`Lang`]. A message is a key, not a sentence: the translation lives in the table next to it, so a
//! missing translation is a compile error rather than an English fallback. The crate is data, like
//! `furnicraft-units`: no serde, no `egui`, no file I/O, and no allocation unless a message carries
//! an argument.
//!
//! The language is workspace settings, never document data. A name the editor generates on its own
//! is fixed in the language that created it: switching the language never renames a node.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fmt;

/// A language the interface speaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    /// English.
    #[default]
    En,
    /// Russian.
    Ru,
}

impl Lang {
    /// Every language the interface offers, in the order the settings window lists them.
    pub const ALL: [Self; 2] = [Self::En, Self::Ru];

    /// How many languages there are.
    ///
    /// A table is `[&str; Lang::COUNT]`, so adding a variant above turns every message into a
    /// compile error until the new translation is written.
    pub const COUNT: usize = Self::ALL.len();

    /// The position of this language in a message table.
    pub const fn index(self) -> usize {
        match self {
            Self::En => 0,
            Self::Ru => 1,
        }
    }

    /// The ISO 639-1 code, used for a file name or a setting value.
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
        }
    }

    /// The language with this code, if any.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            "ru" => Some(Self::Ru),
            _ => None,
        }
    }
}

/// A counted message: one sentence that agrees with a number.
///
/// English needs two forms and Russian three, so the message carries all three and picks one for
/// the language and the number. `one` is used for exactly one, `few` for Russian's 2–4 and `many`
/// for everything else; English uses `one` and `many` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counted {
    n: usize,
    one: Message,
    few: Message,
    many: Message,
}

impl Counted {
    /// The sentence for `n` in `lang`.
    pub fn text(self, lang: Lang) -> String {
        let form = match lang {
            Lang::En => {
                if self.n == 1 {
                    self.one
                } else {
                    self.many
                }
            }
            Lang::Ru => match (self.n % 100, self.n % 10) {
                (11..=14, _) => self.many,
                (_, 1) => self.one,
                (_, 2..=4) => self.few,
                _ => self.many,
            },
        };
        form.with(lang, [self.n])
    }
}

macro_rules! messages {
    ($( $(#[$meta:meta])* $variant:ident = [$en:expr, $ru:expr] ),* $(,)?) => {
        /// One user-facing string: the key, not a sentence.
        ///
        /// `Message::table` is the whole translation, one entry per [`Lang`]. The variant's name is
        /// the key a translator works from; it is never shown to the user and never persisted.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Message {
            $( $(#[$meta])* $variant, )*
        }

        impl Message {
            /// Every message the interface can draw.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )* ];

            /// The translation table: one entry per language.
            pub const fn table(self) -> &'static [&'static str; Lang::COUNT] {
                match self {
                    $( Self::$variant => &[$en, $ru], )*
                }
            }
        }
    };
}

mod messages;

pub use messages::Message;

impl fmt::Display for Message {
    /// The English sentence: logs, tests and a `thiserror` message stay in one language even when
    /// the interface is Russian. A surface that draws the message calls [`Message::text`] instead.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.text(Lang::En))
    }
}

impl Message {
    /// The sentence in `lang`.
    pub const fn text(self, lang: Lang) -> &'static str {
        self.table()[lang.index()]
    }

    /// The sentence in `lang` with each `{}` replaced by the next argument.
    ///
    /// A table entry is one sentence, so an argument is spliced where the `{}` sits instead of
    /// being concatenated after it. A translation that moves the argument to another place in the
    /// sentence therefore keeps working.
    pub fn with<I, A>(self, lang: Lang, args: I) -> String
    where
        I: IntoIterator<Item = A>,
        A: fmt::Display,
    {
        let mut out = String::with_capacity(self.text(lang).len());
        let mut parts = self.text(lang).split("{}");
        if let Some(first) = parts.next() {
            out.push_str(first);
        }
        for (arg, part) in args.into_iter().zip(parts) {
            use fmt::Write as _;
            let _ = write!(out, "{arg}{part}");
        }
        out
    }

    /// A counted message, picking the one/few/many form of `n` itself.
    pub const fn counted(n: usize, one: Message, few: Message, many: Message) -> Counted {
        Counted { n, one, few, many }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_is_non_empty_in_every_language() {
        for message in Message::ALL {
            for lang in Lang::ALL {
                assert!(!message.text(lang).trim().is_empty(), "{message:?} is empty in {lang:?}");
            }
        }
    }

    #[test]
    fn a_language_table_has_one_entry_per_language() {
        for message in Message::ALL {
            assert_eq!(message.table().len(), Lang::COUNT, "{message:?}");
        }
    }

    #[test]
    fn the_code_round_trips() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code("de"), None);
    }

    #[test]
    fn placeholders_are_filled_in_order() {
        assert_eq!(Message::TestPair.with(Lang::En, [1, 2]), "1 and 2");
        assert_eq!(Message::TestPair.with(Lang::Ru, ["a", "b"]), "a и b");
    }

    #[test]
    fn a_counted_message_agrees_with_russian_numbers() {
        assert_eq!(Message::test_points(1).text(Lang::Ru), "1 точка");
        assert_eq!(Message::test_points(3).text(Lang::Ru), "3 точки");
        assert_eq!(Message::test_points(5).text(Lang::Ru), "5 точек");
        assert_eq!(Message::test_points(11).text(Lang::Ru), "11 точек");
        assert_eq!(Message::test_points(21).text(Lang::Ru), "21 точка");
        assert_eq!(Message::test_points(1).text(Lang::En), "1 point");
        assert_eq!(Message::test_points(2).text(Lang::En), "2 points");
    }
}
