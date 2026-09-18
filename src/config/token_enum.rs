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

/// Generate a string-keyed config enum from a `Variant => "token", "Label"`
/// table. `default <Variant>;` names the fallback for an unrecognized token, and
/// a variant may list retired spellings that parse but are never written back.
macro_rules! token_enum {
    (
        $(#[$emeta:meta])*
        $vis:vis enum $name:ident {
            default $default:ident;
            $(
                $(#[$vmeta:meta])*
                $variant:ident => $token:literal $(| $alias:literal)*, $label:literal,
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

            /// Parse leniently: a case- and whitespace-insensitive match on the token
            /// or any of its aliases, falling back to the default so a typo in a
            /// hand-edited config can't break the whole parse.
            pub fn from_value(s: &str) -> Self {
                let s = s.trim();
                $(
                    if s.eq_ignore_ascii_case($token)
                        $( || s.eq_ignore_ascii_case($alias) )*
                    {
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

        // One test module per invocation (fine while each file declares one
        // enum; a second invocation fails loudly on the duplicate name).
        #[cfg(test)]
        mod token_round_trip {
            /// Every variant parses back from its own token (a duplicated token
            /// or alias would resolve to the wrong variant), and an unknown
            /// token falls back to the default — the lenient parse contract.
            #[test]
            fn every_variant_round_trips() {
                $(
                    assert_eq!(
                        super::$name::from_value(super::$name::$variant.as_str()),
                        super::$name::$variant,
                    );
                )+
                assert_eq!(
                    super::$name::from_value("not-a-real-token"),
                    <super::$name as ::core::default::Default>::default(),
                );
            }
        }
    };
}

pub(crate) use token_enum;
