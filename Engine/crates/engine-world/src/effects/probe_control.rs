//! Probe capture requests use ordinary typed component commands and persistence.
use super::EffectKind;
use crate::{EntityId, SceneWorld, WorldCommand, WorldError};
impl SceneWorld {
    /// Requests a new reflection capture without destroying its scene entity.
    /// The revision is consumed asynchronously by the renderer on its next frame,
    /// including for a baked probe without an assigned cubemap. Repeated requests
    /// before a render coalesce into the latest revision.
    /// # Errors
    /// Returns missing entity/component, wrong effect type, exhausted revision,
    /// or invalid authored component errors without changing the previous value.
    pub fn request_reflection_probe_refresh(
        &mut self,
        entity: EntityId,
    ) -> Result<u64, WorldError> {
        let mut effect = self
            .effect(entity)?
            .ok_or_else(|| WorldError::InvalidArgument("entity has no reflection probe".into()))?;
        let EffectKind::ReflectionProbe(probe) = &mut effect.kind else {
            return Err(WorldError::InvalidArgument(
                "effect is not a reflection probe".into(),
            ));
        };
        probe.refresh_revision = probe.refresh_revision.checked_add(1).ok_or_else(|| {
            WorldError::InvalidArgument("probe refresh revision exhausted".into())
        })?;
        let revision = probe.refresh_revision;
        effect.validate().map_err(WorldError::InvalidArgument)?;
        self.apply_commands(&[WorldCommand::SetEffect {
            entity,
            value: Some(Box::new(effect)),
        }])?;
        Ok(revision)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EntitySnapshot,
        effects::{EffectComponent, ProbeRefresh, ReflectionProbe},
    };
    #[test]
    fn refresh_is_typed_atomic_and_extracted_for_an_unassigned_baked_probe() {
        let mut world = SceneWorld::new();
        let probe = EntitySnapshot {
            effect: Some(EffectComponent::new(EffectKind::ReflectionProbe(
                ReflectionProbe::default(),
            ))),
            ..Default::default()
        };
        let other = EntitySnapshot {
            effect: Some(EffectComponent::default()),
            ..Default::default()
        };
        let (id, other_id) = (probe.id, other.id);
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(probe)),
                WorldCommand::Spawn(Box::new(other)),
            ])
            .unwrap();
        let before = world.effect(other_id).unwrap();
        assert!(world.request_reflection_probe_refresh(other_id).is_err());
        assert_eq!(world.effect(other_id).unwrap(), before);
        assert!(
            world
                .request_reflection_probe_refresh(EntityId::new())
                .is_err()
        );
        assert_eq!(world.request_reflection_probe_refresh(id).unwrap(), 1);
        assert_eq!(world.request_reflection_probe_refresh(id).unwrap(), 2);
        world.propagate_transforms();
        world.update_effects(0.0);
        let extracted = &world.effect_frame().probes[0];
        assert_eq!(extracted.entity, id);
        assert_eq!(extracted.refresh_revision, 2);
        assert_eq!(extracted.refresh, ProbeRefresh::Baked);
    }
    #[test]
    fn interval_and_on_enable_are_valid_serialized_modes() {
        for refresh in [ProbeRefresh::OnEnable, ProbeRefresh::Interval] {
            let effect = EffectComponent::new(EffectKind::ReflectionProbe(ReflectionProbe {
                refresh,
                ..Default::default()
            }));
            effect.validate().unwrap();
            let encoded = ron::ser::to_string(&effect).unwrap();
            assert_eq!(ron::from_str::<EffectComponent>(&encoded).unwrap(), effect);
        }
    }
}
