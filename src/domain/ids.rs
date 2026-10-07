use std::fmt::{self, Display};
use std::str::FromStr;

use crate::{CommonplaceError, Result};

macro_rules! tagged_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(i64);

        impl $name {
            pub fn new(value: i64) -> Result<Self> {
                if value <= 0 {
                    return Err(CommonplaceError::InvalidInput(format!(
                        "{} ID must be positive",
                        $prefix
                    )));
                }
                Ok(Self(value))
            }

            pub const fn value(self) -> i64 {
                self.0
            }

            pub(crate) fn stored(value: i64) -> Result<Self> {
                Self::new(value).map_err(|error| {
                    CommonplaceError::Storage(format!(
                        "invalid stored {} ID {value}: {error}",
                        $prefix
                    ))
                })
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}:{}", $prefix, self.0)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl FromStr for $name {
            type Err = CommonplaceError;

            fn from_str(value: &str) -> Result<Self> {
                let raw = value.strip_prefix(concat!($prefix, ":")).ok_or_else(|| {
                    CommonplaceError::InvalidInput(format!(
                        "expected {}:<positive integer>",
                        $prefix
                    ))
                })?;
                let id = raw.parse::<i64>().map_err(|_| {
                    CommonplaceError::InvalidInput(format!(
                        "expected {}:<positive integer>",
                        $prefix
                    ))
                })?;
                Self::new(id)
            }
        }
    };
}

tagged_id!(DocumentId, "doc");
tagged_id!(RevisionId, "revision");
tagged_id!(PassageId, "passage");
tagged_id!(EntityId, "entity");
tagged_id!(EntityTypeId, "entity-type");
tagged_id!(IdentifierSchemeId, "identifier-scheme");
tagged_id!(PredicateId, "predicate");
tagged_id!(KnowledgeItemId, "knowledge");

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{DocumentId, PassageId};

    #[test]
    fn tagged_ids_round_trip() {
        let id = DocumentId::new(42).expect("valid ID");
        assert_eq!(id.to_string(), "doc:42");
        assert_eq!(DocumentId::from_str("doc:42").expect("parse ID"), id);
    }

    #[test]
    fn tagged_ids_reject_wrong_type_and_non_positive_values() {
        assert!(DocumentId::from_str("passage:42").is_err());
        assert!(PassageId::from_str("passage:0").is_err());
    }
}
