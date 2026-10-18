//! Closed vocabularies. Every enumeration the vault stores is closed (12, the kind schemas): one macro gives each the
//! text the vault writes, parsing that refuses anything else, and the serialised form.

/// A value outside a closed set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{value:?} is not a {set} value")]
pub struct OutsideSet {
    /// The set's type name.
    pub set: &'static str,
    /// The value read.
    pub value: String,
}

/// Declares a closed enumeration: `name { Variant = "text", ... }`.
#[macro_export]
macro_rules! closed_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[allow(missing_docs)]
        $vis enum $name { $($variant),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The value as the vault writes it.
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::OutsideSet;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    _ => Err($crate::OutsideSet { set: stringify!($name), value: s.to_string() }),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl $crate::serde::Serialize for $name {
            fn serialize<S: $crate::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
    };
}
