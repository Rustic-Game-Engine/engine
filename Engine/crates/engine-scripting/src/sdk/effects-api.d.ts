// Serialized scene effects use Rust enum variant names and snake_case fields.
type RusticEffectKind = 'Trail' | 'Particle' | 'Beam' | 'Shockwave' | 'MeshParticle' | 'Ribbon' | 'ForceField' | 'CollisionVolume' | 'ProjectileTracer' | 'Fog' | 'Decal' | 'Weather' | 'WaterRipple' | 'Wind' | 'HeatHaze' | 'LightShaft' | 'FoliageScatter' | 'Dissolve' | 'AnimatedSurface' | 'LensFlare' | 'LightPulse' | 'PostProcess' | 'Outline' | 'CameraShake' | 'ScreenFlash' | 'ReflectionProbe';
type RusticEffectParameter = {Scalar:number} | {Vector:[number,number,number]} | {Color:[number,number,number,number]} | {Boolean:boolean};
interface RusticEffectSoundCue { asset:string; delay:number; volume:number; pitch:number; spatial:boolean; looping:boolean; }
interface RusticEffectParameterBinding { parameter:string; child:number; property_path:string; scale:number; offset:number; }
interface RusticEffectTriggerBinding { source_child:number; event:'Birth'|'Collision'|'Death'|'Completed'; target_child:number; action:'Play'|'Pause'|'Resume'|'Stop'|'Restart'|'Reset'; }
interface RusticEffectGroupDefaults {
  settings:{auto_play:boolean;looping:boolean;duration:number;time_scale:number};
  sound_cues:RusticEffectSoundCue[]; parameters:Record<string,RusticEffectParameter>;
  bindings:RusticEffectParameterBinding[]; trigger_bindings:RusticEffectTriggerBinding[];
}
interface RusticEffectGroup {
  readonly source:string|null; readonly child_entities:string[];
  readonly source_baseline:RusticEffectGroupDefaults|null;
  auto_play:boolean; looping:boolean; duration:number; time_scale:number;
  sound_cues:RusticEffectSoundCue[];
  parameters:Record<string,RusticEffectParameter>;
  bindings:RusticEffectParameterBinding[]; trigger_bindings:RusticEffectTriggerBinding[];
}
interface RusticEffectGroupSettings {
  auto_play?:boolean; looping?:boolean; duration?:number; time_scale?:number;
  sound_cues?:RusticEffectSoundCue[];
  /** A null value removes a named instance override. */
  parameters?:Record<string,RusticEffectParameter|null>;
  bindings?:RusticEffectParameterBinding[]; trigger_bindings?:RusticEffectTriggerBinding[];
}
interface RusticEffectCommon {
  enabled: boolean; visible: boolean; auto_play: boolean; looping: boolean;
  duration: number; delay: number; time_scale: number;
  simulation_space: 'Local' | 'World'; attachment_target: string | null;
  visibility_layers: number; render_distance: number;
  quality: 'Low' | 'Medium' | 'High'; random_seed: number; priority: number;
  parameters:Record<string,RusticEffectParameter>;
  bounds:{center:[number,number,number];half_extents:[number,number,number]}|null;
}
interface RusticEffectComponent {
  common: RusticEffectCommon;
  /** Exactly one variant returned by Effects.types(), e.g. {Particle:{rate:30}}. */
  kind: Partial<Record<RusticEffectKind, Record<string, unknown>>>;
}
interface RusticEffectSettings {
  common?: Omit<Partial<RusticEffectCommon>,'parameters'> & {parameters?:Record<string,RusticEffectParameter|null>};
  /** Patch existing fields only; unknown properties throw an error. */
  kind?: Partial<Record<RusticEffectKind, Record<string, unknown>>>;
}
type RusticEffectEntity = string | RusticObjectPath | null;
declare const Effects: {
  types(): RusticEffectKind[];
  /** Requests a new GPU capture on the next rendered frame; returns its revision. */
  refreshProbe(entity?: RusticEffectEntity): number;
  /** Temporary composite; destroys the instance group after lifetime seconds on the scene clock. */
  spawn(asset: string, position: [number,number,number], lifetime: number, parent?: RusticEffectEntity): string;
  instantiate(asset: string, parent?: RusticEffectEntity, position?: [number,number,number]): string;
  sampleWind(position: [number,number,number], time: number, layers?: number): [number,number,number];
  create(entity: RusticEffectEntity, kind: RusticEffectKind): RusticEffectComponent;
  get(entity?: RusticEffectEntity): RusticEffectComponent | null;
  group(entity?: RusticEffectEntity): RusticEffectGroup | null;
  configureGroup(entity: RusticEffectEntity, settings: RusticEffectGroupSettings): RusticEffectGroup;
  configure(entity: RusticEffectEntity, settings: RusticEffectSettings): RusticEffectComponent;
  state(entity?: RusticEffectEntity): 'Stopped' | 'Playing' | 'Paused' | 'Completed' | 'Disposed' | null;
  play(entity?: RusticEffectEntity): void;
  pause(entity?: RusticEffectEntity): void;
  resume(entity?: RusticEffectEntity): void;
  stop(entity?: RusticEffectEntity): void;
  restart(entity?: RusticEffectEntity): void;
  reset(entity?: RusticEffectEntity): void;
  dispose(entity?: RusticEffectEntity): void;
  // Existing gameplay tween helpers remain available.
  fade(entity: RusticEffectEntity, opacity: number, duration: number, easing?: string): unknown;
  flash(entity: RusticEffectEntity, color: [number,number,number], duration: number, easing?: string): unknown;
  pulse(entity: RusticEffectEntity, scale: [number,number,number], duration: number, easing?: string): unknown;
  shake(entity: RusticEffectEntity, strength: number, duration: number, easing?: string): unknown;
};
