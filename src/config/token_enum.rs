//! A small declarative macro for the config's string-keyed enums.
//!
//! [`ToolbarPosition`], [`CursorMode`], and [`MemoryProfile`] are all the same
//! shape: a handful of unit variants, each with a stable lowercase TOML token and
//! a human label for the settings GUI, parsed leniently — an unknown token falls
//! back to a default rather than failing the whole config parse, so a typo in a
//! hand-edited `retsurf.toml` degrades gracefully. [`token_enum!`] generates the
//! enum plus `as_str` / `from_value` / `CHOICES` and the `Default` / serde impls
//! from one table, so each token is spelled exactly once (the GUI reads
//! `Enum::CHOICES` instead of re-listing them).
//!
//! [`ToolbarPosition`]: crate::config::ToolbarPosition
//! [`CursorMode`]: crate::config::CursorMode
//! [`MemoryProfile`]: crate::config::MemoryProfile

/// Generate a string-keyed config enum from a `Variant => "token", "Label"` table.
///
/// `default <Variant>;` names the fallback returned by [`Default`] and by
/// `from_value` for an unrecognized token. See the [module docs](self).
macro_rules! token_enum {
    (
        $(#[$emeta:meta])*
        $vis:vis enum $name:ident {
            default $default:ident;
            $(
                $(#[$vmeta:meta])*
                $variant:ident => $token:literal, $label:literal,
            )+
        }
    ) => {
        $(#[$emeta])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        $vis enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl $name {
            /// `(label, token)` pairs for the settings GUI, in declaration order.
            // Config-only enums (not shown in the GUI) never read this.
            #[allow(dead_code)]
            pub const CHOICES: &'static [(&'static str, &'static str)] =
                &[ $( ($label, $token), )+ ];

            /// The stable TOML/UI token for this value.
            pub fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $token, )+
                }
            }

            /// Parse leniently: a case- and whitespace-insensitive token match,
            /// falling back to the default so a typo in a hand-edited config can't
            /// break the whole parse.
            pub fn from_value(s: &str) -> Self {
                let s = s.trim();
                $(
                    if s.eq_ignore_ascii_case($token) {
                        return $name::$variant;
                    }
                )+
                $name::$default
            }
        }

        #[allow(clippy::derivable_impls)] // the default lives in the table, not a derive
        impl ::core::default::Default for $name {
            fn default() -> Self {
                $name::$default
            }
        }

        // Serialize through the token so the on-disk form stays the single
        // source of truth in both directions (matches the old `rename_all`).
        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(
                &self,
                s: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        // Deserialize via a string so an unknown value falls back to the default
        // instead of failing the whole config parse.
        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                d: D,
            ) -> ::core::result::Result<Self, D::Error> {
                Ok(Self::from_value(&<String as ::serde::Deserialize>::deserialize(d)?))
            }
        }
    };
}

pub(crate) use token_enum;
