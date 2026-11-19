use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            Serialize,
            Deserialize,
            JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub(crate) fn fresh() -> Self {
                Self(Uuid::new_v4())
            }
            pub fn is_valid(self) -> bool {
                !self.0.is_nil()
            }
            pub fn stable_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
id!(DocumentId);
id!(LayerId);
id!(StateId);
id!(GuideId);
id!(PathId);
