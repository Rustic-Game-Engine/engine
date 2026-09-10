pub use engine_core::{AssetId, EntityId, SceneId, WorldId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! typed_uuid {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a fresh stable identity.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing UUID.
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the underlying UUID.
            pub const fn as_uuid(self) -> Uuid {
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
    };
}

typed_uuid!(
    SceneInstanceId,
    "Stable identity of one additive scene instance."
);

/// Short-lived, generational reference to an entity in one specific live world.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RuntimeEntityRef {
    pub(crate) world: WorldId,
    pub(crate) slot: u32,
    pub(crate) generation: u32,
}

impl RuntimeEntityRef {
    /// World that owns this reference.
    pub const fn world(self) -> WorldId {
        self.world
    }

    /// Opaque slot number, useful for diagnostics only.
    pub const fn slot(self) -> u32 {
        self.slot
    }

    /// Slot generation used to reject stale references.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_ids_round_trip_without_cross_domain_conversion() {
        let instance = SceneInstanceId::new();
        assert_eq!(
            instance.to_string().parse::<SceneInstanceId>().unwrap(),
            instance
        );
        let entity = EntityId::new();
        let scene = SceneId::from(*entity.as_uuid());
        assert_ne!(format!("{scene:?}"), format!("{entity:?}"));
    }
}
