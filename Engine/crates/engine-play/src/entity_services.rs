//! Typed requests shared by all language SDKs; no VM-specific entity logic.
use engine_world::{EntityId, SceneWorld};
use serde::Deserialize;
use serde_json::{Value, json};

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
enum Query {
    #[serde(rename = "tags_add")]
    TagAdd { entity: EntityId, name: String },
    #[serde(rename = "tags_remove")]
    TagRemove { entity: EntityId, name: String },
    #[serde(rename = "tags_find_all")]
    TagFind { name: String },
    #[serde(rename = "groups_create")]
    GroupCreate { name: String },
    #[serde(rename = "groups_add")]
    GroupAdd { entity: EntityId, name: String },
    #[serde(rename = "groups_get_members")]
    GroupMembers { name: String },
    #[serde(rename = "hierarchy_set_parent")]
    Parent {
        entity: EntityId,
        parent: Option<EntityId>,
        #[serde(default = "yes")]
        keep_world: bool,
    },
    #[serde(rename = "hierarchy_get_children")]
    Children { entity: EntityId },
    #[serde(rename = "hierarchy_detach")]
    Detach {
        entity: EntityId,
        #[serde(default = "yes")]
        keep_world: bool,
    },
    #[serde(rename = "entity_destroy")]
    Destroy { entity: EntityId },
    #[serde(rename = "entity_destroy_after")]
    DestroyAfter { entity: EntityId, delay: f64 },
    #[serde(rename = "entity_is_valid")]
    Valid { entity: EntityId },
    #[serde(rename = "components_has")]
    Has { entity: EntityId, name: String },
    #[serde(rename = "components_get_all")]
    Components { entity: EntityId },
    #[serde(rename = "components_find_in_children")]
    FindComponents { entity: EntityId, name: String },
    #[serde(rename = "world_find_by_tag")]
    WorldTag { name: String },
    #[serde(rename = "world_find_in_radius")]
    Radius { center: [f32; 3], radius: f64 },
}

pub(crate) fn handles(op: &str) -> bool {
    matches!(
        op,
        "tags_add"
            | "tags_remove"
            | "tags_find_all"
            | "groups_create"
            | "groups_add"
            | "groups_get_members"
            | "hierarchy_set_parent"
            | "hierarchy_get_children"
            | "hierarchy_detach"
            | "entity_destroy"
            | "entity_destroy_after"
            | "entity_is_valid"
            | "components_has"
            | "components_get_all"
            | "components_find_in_children"
            | "world_find_by_tag"
            | "world_find_in_radius"
    )
}

pub(crate) fn query(world: &mut SceneWorld, value: Value) -> Result<Value, String> {
    let query: Query =
        serde_json::from_value(value).map_err(|e| format!("invalid entity query: {e}"))?;
    evaluate(world, query).map_err(|e| e.to_string())
}

fn evaluate(world: &mut SceneWorld, query: Query) -> Result<Value, engine_world::WorldError> {
    match query {
        Query::TagAdd { entity, name } => Ok(json!(world.add_tag(entity, &name)?)),
        Query::TagRemove { entity, name } => Ok(json!(world.remove_tag(entity, &name)?)),
        Query::TagFind { name } | Query::WorldTag { name } => Ok(json!(world.find_by_tag(&name)?)),
        Query::GroupCreate { name } => Ok(json!(world.create_group(&name)?)),
        Query::GroupAdd { entity, name } => Ok(json!(world.add_to_group(&name, entity)?)),
        Query::GroupMembers { name } => Ok(json!(world.group_members(&name)?)),
        Query::Parent {
            entity,
            parent,
            keep_world,
        } => {
            world.reparent(entity, parent, keep_world)?;
            Ok(Value::Null)
        }
        Query::Detach { entity, keep_world } => {
            world.reparent(entity, None, keep_world)?;
            Ok(Value::Null)
        }
        Query::Children { entity } => Ok(json!(world.children(entity)?)),
        Query::Destroy { entity } => {
            world.destroy_entity(entity)?;
            Ok(Value::Null)
        }
        Query::DestroyAfter { entity, delay } => {
            world.destroy_after(entity, delay)?;
            Ok(Value::Null)
        }
        Query::Valid { entity } => Ok(json!(world.contains(entity))),
        Query::Components { entity } => Ok(json!(world.component_names(entity)?)),
        Query::Has { entity, name } => Ok(json!(world.component_names(entity)?.contains(&name))),
        Query::FindComponents { entity, name } => {
            let mut found = Vec::new();
            for id in world.descendants(entity)? {
                if world.component_names(id)?.contains(&name) {
                    found.push(id);
                }
            }
            Ok(json!(found))
        }
        Query::Radius { center, radius } => Ok(json!(
            world.find_in_radius(glam::Vec3::from_array(center), radius)?
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_world::{EntitySnapshot, WorldCommand};

    #[test]
    fn typed_requests_reject_invalid_shapes_without_mutation() {
        let mut world = SceneWorld::new();
        let entity = EntitySnapshot::default();
        let id = entity.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
            .unwrap();
        for request in [
            json!({"op":"tags_add","entity":id,"name":false}),
            json!({"op":"tags_add","entity":"bad","name":"enemy"}),
            json!({"op":"tags_add","entity":id,"name":"enemy","extra":true}),
            json!({"op":"entity_destroy_after","entity":id,"delay":-1}),
            json!({"op":"world_find_in_radius","center":[0,0],"radius":1}),
            json!({"op":"hierarchy_set_parent","entity":id,"parent":id}),
        ] {
            assert!(query(&mut world, request).is_err());
            assert!(world.contains(id));
            assert!(world.find_by_tag("enemy").unwrap().is_empty());
        }
        assert_eq!(
            query(
                &mut world,
                json!({"op":"entity_is_valid","entity":EntityId::new()})
            )
            .unwrap(),
            json!(false)
        );
    }

    #[test]
    fn component_queries_search_descendants_and_not_the_root() {
        let mut world = SceneWorld::new();
        let root = EntitySnapshot::default();
        let child = EntitySnapshot {
            parent: Some(root.id),
            name: Some("child".into()),
            ..EntitySnapshot::default()
        };
        let (r, c) = (root.id, child.id);
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(root)),
                WorldCommand::Spawn(Box::new(child)),
            ])
            .unwrap();
        assert_eq!(
            query(
                &mut world,
                json!({"op":"components_has","entity":c,"name":"rustic.name"})
            )
            .unwrap(),
            json!(true)
        );
        assert_eq!(
            query(
                &mut world,
                json!({"op":"components_find_in_children","entity":r,"name":"rustic.transform"})
            )
            .unwrap(),
            json!([c])
        );
        assert_eq!(
            query(&mut world, json!({"op":"components_get_all","entity":c})).unwrap(),
            json!([
                "rustic.name",
                "rustic.parent",
                "rustic.part_attributes",
                "rustic.transform"
            ])
        );
    }
}
