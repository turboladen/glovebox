//! Vocabularies: values a column takes from a fixed list.
//!
//! [`vocabulary!`] defines one as an enum whose variants each carry the text
//! that is stored, sent in tool arguments and printed in tool output.
//! [`parse`] is the only reader of a vocabulary value typed as text.

use crate::error::{DomainError, DomainResult};

/// A value from a fixed list, with the text each value is written as.
pub trait Vocabulary: Copy + 'static {
    /// Every value, in the order the list is shown.
    const ALL: &'static [Self];

    /// The text this value is stored and printed as.
    fn as_str(self) -> &'static str;
}

/// A text that is not in a vocabulary's list.
#[derive(Debug, thiserror::Error)]
#[error("'{text}' is not valid. Must be one of: {allowed}")]
pub struct Unlisted {
    text: String,
    /// The whole list as text, such as `self, shop`.
    allowed: String,
}

/// The value written as `text`. The match is exact: case and spaces count.
// `parse` is the reader for text someone typed. This one exists for the
// `FromStr` that reading a column needs, where no field name applies.
#[doc(hidden)]
pub fn from_text<V: Vocabulary>(text: &str) -> Result<V, Unlisted> {
    let found = V::ALL.iter().copied().find(|v| v.as_str() == text);
    found.ok_or_else(|| {
        let texts: Vec<&str> = V::ALL.iter().map(|v| v.as_str()).collect();
        Unlisted {
            text: text.to_owned(),
            allowed: texts.join(", "),
        }
    })
}

/// Reads the value of `field` from text someone typed.
///
/// # Errors
///
/// `DomainError::Invalid` naming `field` and listing the allowed values when
/// `text` is not one of them. The match is exact: case and spaces count.
pub fn parse<V: Vocabulary>(field: &str, text: &str) -> DomainResult<V> {
    from_text(text).map_err(|unlisted| DomainError::invalid(field, unlisted.to_string()))
}

/// [`parse`] for a field that may be left empty: `None` stays `None`.
///
/// # Errors
///
/// As [`parse`].
pub fn parse_optional<V: Vocabulary>(field: &str, text: Option<&str>) -> DomainResult<Option<V>> {
    text.map(|t| parse(field, t)).transpose()
}

/// Defines a [`Vocabulary`] enum from `Variant => "text"` pairs.
///
/// The enum is stored as its text in a TEXT column, serializes to and from
/// the same text, and implements `Display` and `FromStr` over it.
// The enum derives `DeriveValueType`, not `DeriveActiveEnum`: the latter
// builds an identifier from each stored text and does not compile for
// `"self"`, which three vocabularies store. A stored value outside the list
// fails the read of its row, so the vocabulary_values migration refuses to
// finish while a column holds one.
#[macro_export]
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash,
            sea_orm::DeriveValueType, serde::Serialize, serde::Deserialize,
        )]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        #[sea_orm(value_type = "String")]
        $vis enum $name {
            $( $(#[$vmeta])* #[serde(rename = $text)] $variant, )+
        }

        impl $name {
            /// The text this value is stored and printed as.
            #[must_use]
            $vis const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $text, )+
                }
            }
        }

        impl $crate::vocab::Vocabulary for $name {
            const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];

            fn as_str(self) -> &'static str {
                Self::as_str(self)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(Self::as_str(*self))
            }
        }

        // A key column needs this, and a value never comes from a row id.
        impl sea_orm::TryFromU64 for $name {
            fn try_from_u64(_: u64) -> Result<Self, sea_orm::DbErr> {
                Err(sea_orm::DbErr::ConvertFromU64(stringify!($name)))
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::vocab::Unlisted;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                $crate::vocab::from_text(text)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DomainError;

    vocabulary! {
        enum Sample {
            Owner => "self",
            ThirdParty => "third_party",
        }
    }

    #[test]
    fn a_value_is_written_as_its_text_everywhere() {
        assert_eq!(Sample::Owner.as_str(), "self");
        assert_eq!(Sample::ThirdParty.to_string(), "third_party");
        assert_eq!(serde_json::to_string(&Sample::Owner).unwrap(), "\"self\"");
        let read: Sample = serde_json::from_str("\"third_party\"").unwrap();
        assert_eq!(read, Sample::ThirdParty);
    }

    #[test]
    fn parse_reads_a_listed_value() {
        assert_eq!(parse::<Sample>("paid_by", "self").unwrap(), Sample::Owner);
        assert_eq!(parse_optional::<Sample>("paid_by", None).unwrap(), None);
    }

    #[test]
    fn from_str_reads_the_same_text_and_names_no_field() {
        assert_eq!("self".parse::<Sample>().unwrap(), Sample::Owner);
        let err = "bogus".parse::<Sample>().unwrap_err().to_string();
        assert_eq!(
            err,
            "'bogus' is not valid. Must be one of: self, third_party"
        );
    }

    #[test]
    fn parse_names_the_field_and_the_allowed_values() {
        for text in ["bogus", "Self", " self", ""] {
            let Err(DomainError::Invalid { field, message }) = parse::<Sample>("paid_by", text)
            else {
                panic!("'{text}' should be refused");
            };
            assert_eq!(field, "paid_by");
            assert!(message.contains("self, third_party"), "{message}");
        }
    }
}
