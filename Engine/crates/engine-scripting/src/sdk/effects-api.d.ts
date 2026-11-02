// Serialized scene effects use Rust enum variant names and snake_case fields.
type RusticEffectKind = 'Trail' | 'Particle' | 'Beam' | 'Shockwave' | 'MeshParticle' | 'Ribbon' | 'ForceField' | 'CollisionVolume' | 'ProjectileTracer' | 'Fog' | 'Decal' | 'Weather' | 'WaterRipple' | 'Wind' | 'HeatHaze' | 'LightShaft' | 'FoliageScatter' | 'Dissolve' | 'AnimatedSurface' | 'LensFlare' | 'LightPulse' | 'PostProcess' | 'Outline' | 'CameraShake' | 'ScreenFlash' | 'ReflectionProbe';
interface RusticEffectCommon {
  enabled: boolean; visible: boolean; auto_play: boolean; looping: boolean;
  duration: number; delay: number; time_scale: number;
  simulation_space: 'Local' | 'World'; attachment_target: string | null;
  visibility_layers: number; render_distance: number;
  quality: 'Low' | 'Medium' | 'High'; random_seed: number; priority: number;
}
interface RusticEffectComponent {
  common: RusticEffectCommon;
  /** Exactly one variant returned by Effects.types(), e.g. {Particle:{rate:30}}. */
  kind: Partial<Record<RusticEffectKind, Record<string, unknown>>>;
}
interface RusticEffectSettings {
  common?: Partial<RusticEffectCommon>;
  /** Patch existing fields only; unknown properties throw an error. */
  kind?: Partial<Record<RusticEffectKind, Record<string, unknown>>>;
}
type RusticEffectEntity = string | RusticObjectPath | null;
declare const Effects: {
  types(): RusticEffectKind[];
  /** Temporary composite; destroys the instance group after lifetime seconds on the scene clock. */
  spawn(asset: string, position: [number,number,number], lifetime: number, parent?: RusticEffectEntity): string;
  instantiate(asset: string, parent?: RusticEffectEntity, position?: [number,number,number]): string;
  sampleWind(position: [number,number,number], time: number, layers?: number): [number,number,number];
  create(entity: RusticEffectEntity, kind: RusticEffectKind): RusticEffectComponent;
  get(entity?: RusticEffectEntity): RusticEffectComponent | null;
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
