use super::{
    Action, Delivery, EntityId, GameplayWorld, Owner, Payload, Playback, Signal, Signals, Target,
    Timing, Value, path,
};
use glam::{DMat3, DQuat, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Paused,
    Finished,
    Cancelled,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationState {
    pub handle: String,
    pub status: Status,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
#[allow(
    clippy::large_enum_variant,
    reason = "Requests are consumed at the API boundary; action payloads are already bounded and do not occupy a retained queue"
)]
pub enum Request {
    Start {
        handle: String,
        action: Action,
        #[serde(default)]
        playback: Playback,
        #[serde(default)]
        on_finished: Option<String>,
    },
    Pause {
        handle: String,
    },
    Resume {
        handle: String,
    },
    Cancel {
        handle: String,
    },
    Reverse {
        handle: String,
    },
    Speed {
        handle: String,
        speed: f64,
    },
    Loop {
        handle: String,
        looping: bool,
    },
    Connect {
        token: String,
        signal: Signal,
        #[serde(default)]
        once: bool,
    },
    Disconnect {
        token: String,
    },
    Emit {
        signal: Signal,
        #[serde(default)]
        arguments: Vec<Payload>,
    },
}
struct Node {
    action: Action,
    clip_player: Option<super::animation::ClipPlayer>,
    base_pose: std::collections::BTreeMap<String, Value>,
    children: Vec<Node>,
    index: usize,
    elapsed: f64,
    duration: Option<f64>,
    from: Option<Value>,
    to: Option<Value>,
    table: Vec<(f64, f64)>,
    done: bool,
    reverse: bool,
}
impl Node {
    fn new(action: Action) -> Self {
        let children = match &action {
            Action::Sequence { actions } | Action::Parallel { actions } => {
                actions.iter().cloned().map(Self::new).collect()
            }
            _ => Vec::new(),
        };
        Self {
            action,
            clip_player: None,
            base_pose: std::collections::BTreeMap::new(),
            children,
            index: 0,
            elapsed: 0.0,
            duration: None,
            from: None,
            to: None,
            table: Vec::new(),
            done: false,
            reverse: false,
        }
    }
    fn set_clip_loop(&mut self, looping: bool) {
        if let Action::Animation { options, .. } = &mut self.action {
            options.looping = looping;
            if let Some(player) = &mut self.clip_player {
                player.options.looping = looping;
            }
        }
    }
    fn set_speed(&mut self, speed: f64) -> bool {
        if let Action::Animation { options, .. } = &mut self.action {
            options.speed = speed;
            if let Some(player) = &mut self.clip_player {
                player.options.speed = speed;
            }
            true
        } else {
            false
        }
    }
    fn reversible(&self) -> bool {
        matches!(
            self.action,
            Action::Value { .. }
                | Action::Tween { .. }
                | Action::Move { .. }
                | Action::LookAt { .. }
                | Action::Path { .. }
                | Action::Orbit { .. }
                | Action::Wait { .. }
        )
    }
    fn reset(&mut self, ping_pong: bool) {
        self.elapsed = 0.0;
        self.done = false;
        self.index = 0;
        self.clip_player = None;
        if ping_pong {
            self.reverse = !self.reverse;
        }
        for child in &mut self.children {
            child.reset(ping_pong);
        }
        // Keep captured start values across repeats, but sequence children capture lazily initially.
    }
    /// Returns unconsumed frame seconds when finished.
    #[allow(
        clippy::too_many_lines,
        reason = "Cooperative action-tree advancement shares one timing and callback budget"
    )]
    fn advance(
        &mut self,
        dt: f64,
        world: &mut dyn GameplayWorld,
        owner: Owner,
        queue: &mut VecDeque<Delivery>,
        budget: &mut usize,
    ) -> Result<f64, String> {
        if self.done {
            return Ok(dt);
        }
        if *budget == 0 {
            return Err("action step budget exceeded".into());
        }
        *budget -= 1;
        match &self.action {
            Action::Sequence { .. } => {
                let mut remaining = dt;
                while self.index < self.children.len() {
                    remaining = self.children[self.index]
                        .advance(remaining, world, owner, queue, budget)?;
                    if !self.children[self.index].done {
                        return Ok(0.0);
                    }
                    self.index += 1;
                }
                self.done = true;
                return Ok(remaining);
            }
            Action::Parallel { .. } => {
                let mut remaining = dt;
                for child in &mut self.children {
                    remaining = remaining.min(child.advance(dt, world, owner, queue, budget)?);
                }
                self.done = self.children.iter().all(|n| n.done);
                return Ok(if self.done { remaining } else { 0.0 });
            }
            Action::Callback { token } => {
                if queue.len() >= 4096 {
                    return Err("callback delivery limit reached".into());
                }
                queue.push_back(Delivery {
                    owner,
                    token: token.clone(),
                    arguments: Vec::new(),
                    source: None,
                });
                self.done = true;
                return Ok(dt);
            }
            _ => {}
        }
        if matches!(self.action, Action::AnimationRef { .. }) {
            return Err("animation clip reference must be resolved by the host".into());
        }
        if let Action::Animation {
            entity,
            clip,
            options,
            marker_token,
            blend_source,
            mask,
        } = &self.action
        {
            if self.clip_player.is_none() {
                for track in &clip.tracks {
                    if !track_in_mask(mask, &track.target) {
                        continue;
                    }
                    let target = world.animation_target(*entity, &track.target)?;
                    self.base_pose
                        .insert(track.target.clone(), world.read(&target)?);
                }
                self.clip_player = Some(super::animation::ClipPlayer::new(*options)?);
            }
            let player = self.clip_player.as_mut().ok_or("missing clip player")?;
            let remaining = (clip.duration - player.time) / player.options.speed;
            let consumed = if player.options.looping {
                dt
            } else {
                dt.min(remaining.max(0.0))
            };
            let frame = player.tick_validated(clip, consumed)?;
            let base = if let Some(source) = blend_source {
                source.sample(player.time.min(source.duration))?
            } else if options.layered {
                self.base_pose
                    .keys()
                    .map(|track| {
                        Ok((
                            track.clone(),
                            world.read(&world.animation_target(*entity, track)?)?,
                        ))
                    })
                    .collect::<Result<_, String>>()?
            } else {
                self.base_pose.clone()
            };
            let reference = if options.additive {
                clip.sample(0.0)?
            } else {
                std::collections::BTreeMap::new()
            };
            for (track, value) in &frame.pose {
                if !track_in_mask(mask, track) {
                    continue;
                }
                let target = world.animation_target(*entity, track)?;
                let output = if options.additive {
                    super::animation::additive(
                        world.read(&target)?,
                        *value,
                        *reference.get(track).ok_or("additive reference missing")?,
                        frame.weight,
                    )?
                } else {
                    let from = base.get(track).copied().unwrap_or(world.read(&target)?);
                    from.interpolate(*value, frame.weight)?
                };
                world.write(&target, output)?;
            }
            if let Some(token) = marker_token {
                for marker in frame.markers {
                    if queue.len() >= 4096 {
                        return Err("callback delivery limit reached".into());
                    }
                    queue.push_back(Delivery {
                        owner,
                        token: token.clone(),
                        arguments: vec![Payload::String(marker)],
                        source: Some(*entity),
                    });
                }
            }
            self.done = frame.finished;
            return Ok(if self.done { dt - consumed } else { 0.0 });
        }
        self.initialize(world)?;
        let duration = self.duration.unwrap_or(0.0);
        let consumed = dt.min((duration - self.elapsed).max(0.0));
        self.elapsed += consumed;
        let fraction = if duration <= f64::EPSILON {
            1.0
        } else {
            (self.elapsed / duration).clamp(0.0, 1.0)
        };
        let fraction = if self.reverse {
            1.0 - fraction
        } else {
            fraction
        };
        self.apply_sample(fraction, world, owner, queue)?;
        self.done = self.elapsed >= duration;
        Ok(if self.done { dt - consumed } else { 0.0 })
    }
    fn initialize(&mut self, world: &mut dyn GameplayWorld) -> Result<(), String> {
        if self.duration.is_none() {
            self.duration = Some(match &self.action {
                Action::Value {
                    from, to, timing, ..
                } => timing.duration(from.distance(*to)?),
                Action::Tween {
                    target,
                    from,
                    to,
                    timing,
                    ..
                } => {
                    let start = from.map_or_else(|| world.read(target), Ok)?;
                    start.validate()?;
                    self.from = Some(start);
                    timing.duration(start.distance(*to)?)
                }
                Action::Move {
                    entity,
                    offset,
                    timing,
                    ..
                } => {
                    let from = world.read(&Target {
                        entity: *entity,
                        property: "Position".into(),
                    })?;
                    let Value::Vector(v) = from else {
                        return Err("move requires a vector position".into());
                    };
                    let to = Value::Vector(
                        (DVec3::from_array(v) + DVec3::from_array(*offset)).to_array(),
                    );
                    self.from = Some(from);
                    self.to = Some(to);
                    timing.duration(from.distance(to)?)
                }
                Action::LookAt {
                    entity,
                    position,
                    duration,
                    ..
                } => {
                    let Value::Vector(v) = world.read(&Target {
                        entity: *entity,
                        property: "Position".into(),
                    })?
                    else {
                        return Err("lookAt requires position".into());
                    };
                    let rotation =
                        look_rotation(DVec3::from_array(*position) - DVec3::from_array(v))
                            .ok_or("lookAt target coincides with object position")?;
                    self.from = Some(world.read(&Target {
                        entity: *entity,
                        property: "Rotation".into(),
                    })?);
                    self.to = Some(Value::Rotation(rotation.to_array()));
                    *duration
                }
                Action::Shake {
                    entity, duration, ..
                }
                | Action::Follow {
                    entity, duration, ..
                } => {
                    self.from = Some(world.read(&Target {
                        entity: *entity,
                        property: "Position".into(),
                    })?);
                    *duration
                }
                Action::Path { path, timing, .. } => {
                    self.table = path.arc_lengths();
                    timing.duration(self.table.last().map_or(0.0, |v| v.1))
                }
                Action::Orbit { duration, .. } | Action::Wait { duration } => *duration,
                _ => unreachable!(),
            });
        }
        if self.duration.is_some_and(|d| !d.is_finite() || d < 0.0) {
            return Err("action duration overflowed".into());
        }
        Ok(())
    }
    #[allow(
        clippy::too_many_lines,
        reason = "Exhaustive property sampling of the closed action protocol"
    )]
    fn apply_sample(
        &mut self,
        fraction: f64,
        world: &mut dyn GameplayWorld,
        owner: Owner,
        queue: &mut VecDeque<Delivery>,
    ) -> Result<(), String> {
        match &self.action {
            Action::Value {
                from,
                to,
                easing,
                token,
                ..
            } => {
                if queue.len() >= 4096 {
                    return Err("callback delivery limit reached".into());
                }
                queue.push_back(Delivery {
                    owner,
                    token: token.clone(),
                    arguments: vec![from.interpolate(*to, easing.sample(fraction))?.into()],
                    source: None,
                });
            }
            Action::Tween {
                target, to, easing, ..
            } => world.write(
                target,
                self.from
                    .ok_or("missing tween start")?
                    .interpolate(*to, easing.sample(fraction))?,
            )?,
            Action::Move { entity, easing, .. } | Action::LookAt { entity, easing, .. } => {
                let property = if matches!(self.action, Action::Move { .. }) {
                    "Position"
                } else {
                    "Rotation"
                };
                world.write(
                    &Target {
                        entity: *entity,
                        property: property.into(),
                    },
                    self.from
                        .ok_or("missing start")?
                        .interpolate(self.to.ok_or("missing target")?, easing.sample(fraction))?,
                )?;
            }
            Action::Shake {
                entity,
                strength,
                easing,
                ..
            } => {
                let Value::Vector(base) = self.from.ok_or("missing shake start")? else {
                    return Err("shake requires vector position".into());
                };
                let falloff = if fraction >= 1.0 {
                    0.0
                } else {
                    1.0 - easing.sample(fraction)
                };
                let phase = self.elapsed * 37.0;
                let offset = DVec3::new(phase.sin(), (phase * 1.37).sin(), (phase * 0.73).sin())
                    * strength
                    * falloff;
                world.write(
                    &Target {
                        entity: *entity,
                        property: "Position".into(),
                    },
                    Value::Vector((DVec3::from_array(base) + offset).to_array()),
                )?;
            }
            Action::Path {
                entity,
                path,
                easing,
                orient_to_path,
                timing,
                ..
            } => {
                let eased = easing.sample(fraction);
                let t = if matches!(timing, Timing::Speed { .. }) {
                    path::distance_parameter(&self.table, eased)
                } else {
                    eased.clamp(0.0, 1.0)
                };
                let t = path.traversal_parameter(t);
                world.write(
                    &Target {
                        entity: *entity,
                        property: "Position".into(),
                    },
                    Value::Vector(path.sample(t).to_array()),
                )?;
                if *orient_to_path {
                    let (a, b) = if self.reverse {
                        ((t + 0.0001).min(1.0), (t - 0.0001).max(0.0))
                    } else {
                        ((t - 0.0001).max(0.0), (t + 0.0001).min(1.0))
                    };
                    if let Some(q) = look_rotation(path.sample(b) - path.sample(a)) {
                        world.write(
                            &Target {
                                entity: *entity,
                                property: "Rotation".into(),
                            },
                            Value::Rotation(q.to_array()),
                        )?;
                    }
                }
            }
            Action::Follow {
                entity,
                target,
                offset,
                easing,
                ..
            } => {
                let Value::Vector(target) = world.read(&Target {
                    entity: *target,
                    property: "Position".into(),
                })?
                else {
                    return Err("follow target is not a position".into());
                };
                let to = Value::Vector(
                    (DVec3::from_array(target) + DVec3::from_array(*offset)).to_array(),
                );
                world.write(
                    &Target {
                        entity: *entity,
                        property: "Position".into(),
                    },
                    self.from
                        .ok_or("missing follow start")?
                        .interpolate(to, easing.sample(fraction))?,
                )?;
            }
            Action::Orbit {
                entity,
                center,
                radius,
                turns,
                easing,
                ..
            } => {
                let angle = easing.sample(fraction) * turns * std::f64::consts::TAU;
                let value = DVec3::from_array(*center)
                    + DVec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
                world.write(
                    &Target {
                        entity: *entity,
                        property: "Position".into(),
                    },
                    Value::Vector(value.to_array()),
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}
fn track_in_mask(mask: &[String], track: &str) -> bool {
    mask.is_empty()
        || mask.iter().any(|prefix| {
            track == prefix
                || track
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
}
/// Engine cameras/objects face their local -Z axis; Y is the preferred up vector.
pub fn look_rotation(direction: DVec3) -> Option<DQuat> {
    let forward = direction.try_normalize()?;
    let up = if forward.dot(DVec3::Y).abs() > 0.999 {
        DVec3::X
    } else {
        DVec3::Y
    };
    let right = forward.cross(up).normalize();
    let up = right.cross(forward);
    Some(DQuat::from_mat3(&DMat3::from_cols(right, up, -forward)).normalize())
}
struct Operation {
    nodes: usize,
    serial: u64,
    node: Node,
    playback: Playback,
    loops: u32,
    status: Status,
    error: Option<String>,
    finished: Option<String>,
    speed: f64,
}
/// A scene's shared deterministic action/signal scheduler.
pub struct GameplayRuntime {
    operations: BTreeMap<(Owner, String), Operation>,
    callbacks: VecDeque<Delivery>,
    terminal: VecDeque<(Owner, String)>,
    pub signals: Signals,
    pub paused: bool,
    time_scale: f64,
    next_serial: u64,
}
impl Default for GameplayRuntime {
    fn default() -> Self {
        Self {
            operations: BTreeMap::new(),
            callbacks: VecDeque::new(),
            terminal: VecDeque::new(),
            signals: Signals::default(),
            paused: false,
            time_scale: 1.0,
            next_serial: 0,
        }
    }
}
impl GameplayRuntime {
    /// # Errors
    /// Rejects non-finite or negative time scales.
    pub fn set_time_scale(&mut self, scale: f64) -> Result<(), String> {
        if !scale.is_finite() || scale < 0.0 {
            return Err("time scale must be finite and non-negative".into());
        }
        self.time_scale = scale;
        Ok(())
    }
    pub fn time_scale(&self) -> f64 {
        self.time_scale
    }
    /// # Errors
    /// Rejects invalid deltas and scaled time overflow.
    pub fn scaled_delta(&self, delta: f64) -> Result<f64, String> {
        if !delta.is_finite() || delta < 0.0 {
            return Err("invalid frame delta".into());
        }
        let scaled = delta * self.time_scale;
        if !scaled.is_finite() {
            return Err("scaled frame delta overflowed".into());
        }
        Ok(if self.paused { 0.0 } else { scaled })
    }
    /// # Errors
    /// Rejects invalid actions, stale handles, unsupported controls, or capacity exhaustion.
    #[allow(
        clippy::too_many_lines,
        clippy::items_after_statements,
        reason = "Protocol dispatch and bounded callback-token collection stay together"
    )]
    pub fn request(&mut self, owner: Owner, request: Request) -> Result<(), String> {
        match request {
            Request::Start {
                handle,
                action,
                playback,
                on_finished,
            } => {
                action.validate()?;
                if action.has_clip_references() {
                    return Err(
                        "host must resolve animation asset references before scheduling".into(),
                    );
                }
                let nodes = action.node_count();
                if self.operations.values().map(|op| op.nodes).sum::<usize>() + nodes > 65_536 {
                    return Err("aggregate action node limit reached".into());
                }
                if on_finished.as_ref().is_some_and(|token| token.len() > 128) {
                    return Err("completion token exceeds 128 bytes".into());
                }
                if playback.ping_pong && !Node::new(action.clone()).reversible() {
                    return Err("ping-pong requires a tween, path, orbit, or wait".into());
                }
                if handle.is_empty() || handle.len() > 128 {
                    return Err("invalid operation handle".into());
                }
                if self.operations.len() >= 8192 {
                    return Err("operation limit reached".into());
                }
                let key = (owner, handle);
                if self.operations.contains_key(&key) {
                    return Err("operation handle already exists".into());
                }
                self.next_serial = self
                    .next_serial
                    .checked_add(1)
                    .ok_or("operation serial exhausted")?;
                let mut node = Node::new(action);
                if playback.looping {
                    node.set_clip_loop(true);
                }
                self.operations.insert(
                    key,
                    Operation {
                        nodes,
                        serial: self.next_serial,
                        node,
                        playback,
                        loops: 0,
                        status: Status::Running,
                        error: None,
                        finished: on_finished,
                        speed: 1.0,
                    },
                );
            }
            Request::Pause { handle } => {
                let op = self.operation(owner, &handle)?;
                if op.status == Status::Running {
                    op.status = Status::Paused;
                }
            }
            Request::Resume { handle } => {
                let op = self.operation(owner, &handle)?;
                if op.status == Status::Paused {
                    op.status = Status::Running;
                }
            }
            Request::Cancel { handle } => {
                let op = self.operation(owner, &handle)?;
                if matches!(op.status, Status::Running | Status::Paused) {
                    op.status = Status::Cancelled;
                    self.terminal.push_back((owner, handle.clone()));
                }
                let mut tokens = Vec::new();
                fn collect(action: &Action, tokens: &mut Vec<String>) {
                    match action {
                        Action::Animation {
                            marker_token: Some(token),
                            ..
                        }
                        | Action::Callback { token }
                        | Action::Value { token, .. } => {
                            tokens.push(token.clone());
                        }
                        Action::Sequence { actions } | Action::Parallel { actions } => {
                            for a in actions {
                                collect(a, tokens);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(op) = self.operations.get(&(owner, handle.clone())) {
                    collect(&op.node.action, &mut tokens);
                }
                self.callbacks
                    .retain(|d| d.owner != owner || !tokens.contains(&d.token));
                // Cancel queued completion tokens too.
                if let Some(token) = self
                    .operations
                    .get(&(owner, handle))
                    .and_then(|o| o.finished.as_ref())
                {
                    self.callbacks
                        .retain(|d| d.owner != owner || d.token != *token);
                }
            }
            Request::Reverse { handle } => {
                let op = self.operation(owner, &handle)?;
                if !op.node.reversible() || !matches!(op.status, Status::Running | Status::Paused) {
                    return Err("operation cannot reverse in its current state".into());
                }
                if let Some(duration) = op.node.duration {
                    op.node.elapsed = duration - op.node.elapsed;
                }
                op.node.reverse = !op.node.reverse;
            }
            Request::Speed { handle, speed } => {
                if !speed.is_finite() || speed <= 0.0 {
                    return Err("speed must be finite and positive".into());
                }
                let operation = self
                    .operations
                    .get_mut(&(owner, handle))
                    .ok_or("unknown action handle")?;
                if !operation.node.set_speed(speed) {
                    operation.speed = speed;
                }
            }
            Request::Loop { handle, looping } => {
                let operation = self
                    .operations
                    .get_mut(&(owner, handle))
                    .ok_or("unknown action handle")?;
                operation.playback.looping = looping;
                operation.node.set_clip_loop(looping);
            }
            Request::Connect {
                token,
                signal,
                once,
            } => self.signals.connect(owner, token, signal, once)?,
            Request::Disconnect { token } => self.signals.disconnect(owner, &token),
            Request::Emit { signal, arguments } => self.signals.emit(&signal, arguments)?,
        }
        self.prune();
        Ok(())
    }
    fn operation(&mut self, owner: Owner, handle: &str) -> Result<&mut Operation, String> {
        self.operations
            .get_mut(&(owner, handle.into()))
            .ok_or_else(|| "unknown operation handle".into())
    }
    /// # Errors
    /// Rejects invalid frame time. Individual action errors are recorded in operation state.
    pub fn tick(&mut self, delta: f64, world: &mut dyn GameplayWorld) -> Result<(), String> {
        if !delta.is_finite() || delta < 0.0 {
            return Err("frame delta must be finite and non-negative".into());
        }
        let dt = delta * self.time_scale;
        if !dt.is_finite() {
            return Err("scaled frame delta overflowed".into());
        }
        for entity in self.states_for_entities() {
            if !world.contains(entity) {
                self.destroy_entity(entity);
            }
        }
        if self.paused || dt == 0.0 {
            return Ok(());
        }
        let mut keys: Vec<_> = self
            .operations
            .iter()
            .map(|(key, op)| (op.serial, key.clone()))
            .collect();
        keys.sort_by_key(|k| k.0);
        for (_, key) in keys {
            let Some(op) = self.operations.get_mut(&key) else {
                continue;
            };
            if op.status != Status::Running {
                continue;
            }
            let mut remaining = dt * op.speed;
            if !remaining.is_finite() {
                op.status = Status::Failed;
                op.error = Some("action time overflow".into());
                continue;
            }
            let mut budget = 8192;
            let outcome = (|| -> Result<(), String> {
                loop {
                    remaining = op.node.advance(
                        remaining,
                        world,
                        key.0,
                        &mut self.callbacks,
                        &mut budget,
                    )?;
                    if !op.node.done {
                        break;
                    }
                    let repeat =
                        op.playback.looping || op.playback.repeats.is_none_or(|n| op.loops < n);
                    if !repeat {
                        op.status = Status::Finished;
                        break;
                    }
                    op.loops = op.loops.saturating_add(1);
                    op.node.reset(op.playback.ping_pong);
                    if remaining <= 0.0 {
                        break;
                    }
                    if budget == 0 {
                        return Err("repeat step budget exceeded (zero-duration loop?)".into());
                    }
                }
                Ok(())
            })();
            if let Err(error) = outcome {
                op.status = Status::Failed;
                op.error = Some(error);
            }
            if matches!(op.status, Status::Finished | Status::Failed) {
                self.terminal.push_back(key.clone());
                if op.status == Status::Finished
                    && let Some(token) = &op.finished
                    && self.callbacks.len() < 4096
                {
                    self.callbacks.push_back(Delivery {
                        owner: key.0,
                        token: token.clone(),
                        arguments: Vec::new(),
                        source: None,
                    });
                }
            }
        }
        self.prune();
        Ok(())
    }
    fn prune(&mut self) {
        while self.terminal.len() > 1024 {
            if let Some(key) = self.terminal.pop_front() {
                self.operations.remove(&key);
            }
        }
    }
    #[allow(
        clippy::items_after_statements,
        reason = "Local recursive visitor only serves entity-reference discovery"
    )]
    pub fn states_for_entities(&self) -> Vec<EntityId> {
        let mut result = self.signals.entities();
        for ((owner, _), op) in &self.operations {
            if owner.entity_bound {
                result.push(owner.entity);
            }
            fn visit(action: &Action, result: &mut Vec<EntityId>) {
                match action {
                    Action::Tween { target, .. } => result.push(target.entity),
                    Action::Animation { entity, .. }
                    | Action::Move { entity, .. }
                    | Action::LookAt { entity, .. }
                    | Action::Shake { entity, .. }
                    | Action::Path { entity, .. }
                    | Action::Orbit { entity, .. } => result.push(*entity),
                    Action::Follow { entity, target, .. } => {
                        result.push(*entity);
                        result.push(*target);
                    }
                    Action::Sequence { actions } | Action::Parallel { actions } => {
                        for a in actions {
                            visit(a, result);
                        }
                    }
                    _ => {}
                }
            }
            visit(&op.node.action, &mut result);
        }
        result.sort();
        result.dedup();
        result
    }
    pub fn states(&self, owner: Owner) -> Vec<OperationState> {
        self.operations
            .iter()
            .filter(|(k, _)| k.0 == owner)
            .map(|(k, o)| OperationState {
                handle: k.1.clone(),
                status: o.status,
                error: o.error.clone(),
            })
            .collect()
    }
    pub fn drain_callbacks(&mut self, owner: Owner) -> Vec<Delivery> {
        let mut result = self.signals.drain(owner);
        self.callbacks.retain(|d| {
            if d.owner == owner {
                result.push(d.clone());
                false
            } else {
                true
            }
        });
        result
    }
    pub fn cleanup(&mut self, predicate: impl Fn(Owner) -> bool) {
        self.operations.retain(|k, _| !predicate(k.0));
        self.callbacks.retain(|d| !predicate(d.owner));
        self.terminal.retain(|k| !predicate(k.0));
        self.signals.cleanup(predicate);
    }
    #[allow(
        clippy::items_after_statements,
        reason = "Local recursive visitor only serves lifetime cleanup"
    )]
    pub fn destroy_entity(&mut self, entity: EntityId) {
        let mut tokens = Vec::new();
        fn collect(node: &Node, owner: Owner, tokens: &mut Vec<(Owner, String)>) {
            if let Action::Animation {
                marker_token: Some(token),
                ..
            }
            | Action::Callback { token }
            | Action::Value { token, .. } = &node.action
            {
                tokens.push((owner, token.clone()));
            }
            for child in &node.children {
                collect(child, owner, tokens);
            }
        }
        self.operations.retain(|k, o| {
            let remove =
                (k.0.entity_bound && k.0.entity == entity) || o.node.action.references(entity);
            if remove {
                collect(&o.node, k.0, &mut tokens);
                if let Some(token) = &o.finished {
                    tokens.push((k.0, token.clone()));
                }
            }
            !remove
        });
        self.callbacks
            .retain(|d| !tokens.contains(&(d.owner, d.token.clone())));
        self.callbacks
            .retain(|d| !d.owner.entity_bound || d.owner.entity != entity);
        self.terminal
            .retain(|k| !k.0.entity_bound || k.0.entity != entity);
        self.signals.destroy_entity(entity);
    }
}
