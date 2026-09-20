//! Domain-specific stable UUID identities.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! stable_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new random stable identity.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Borrows the underlying interoperable UUID.
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Returns the underlying interoperable UUID.
            pub const fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }
    };
}

stable_id!(ProjectId, "Stable identity of a project descriptor.");
stable_id!(AssetId, "Stable identity of an imported or native asset.");
stable_id!(SceneId, "Stable identity of a serialized scene resource.");
stable_id!(EntityId, "Stable authoring identity of an entity.");
stable_id!(
    ScriptId,
    "Stable identity of a gameplay script independent of its path."
);
stable_id!(
    WorldId,
    "Stable identity of a world used in cross-world references."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_round_trips_as_text_and_serde() {
        macro_rules! check {
            ($id:expr, $type:ty) => {{
                let id: $type = $id;
                assert_eq!(id.to_string().parse::<$type>().unwrap(), id);
                let ron = ron::to_string(&id).unwrap();
                assert_eq!(ron::from_str::<$type>(&ron).unwrap(), id);
            }};
        }
        check!(ProjectId::new(), ProjectId);
        check!(AssetId::new(), AssetId);
        check!(SceneId::new(), SceneId);
        check!(EntityId::new(), EntityId);
        check!(ScriptId::new(), ScriptId);
        check!(WorldId::new(), WorldId);
    }

    #[test]
    fn domain_ids_are_not_interchangeable() {
        let asset = AssetId::new();
        let entity = EntityId::from(*asset.as_uuid());
        assert_eq!(asset.to_string(), entity.to_string());
        // Their distinct Rust types prevent accidental API interchange at compile time.
        assert_eq!(
            std::any::type_name::<AssetId>(),
            "engine_core::ids::AssetId"
        );
        assert_ne!(
            std::any::type_name::<AssetId>(),
            std::any::type_name::<EntityId>()
        );
    }
}
