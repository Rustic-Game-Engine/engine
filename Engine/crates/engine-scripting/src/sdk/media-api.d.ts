interface RusticCapturePacket {
  samples: number[];
  sampleRate: number;
  channels: number;
  active: boolean;
  droppedSamples: number;
}
interface RusticCaptureHandle {
  id: string;
  read(): RusticCapturePacket;
  stop(): void;
  unload(): void;
}
interface RusticCaptureApi {
  startRecording(path?: string | null, options?: { source?: "game" | "microphone" }): RusticCaptureHandle;
  stopRecording(handle?: RusticCaptureHandle | null): void;
  startHotMic(): RusticCaptureHandle;
  stopHotMic(handle?: RusticCaptureHandle | null): void;
}
interface RusticAudioOptions {
  volume?: number;
  pitch?: number;
  loop?: boolean;
  position?: [number, number, number];
}
interface RusticAudioVoice {
  id: string;
  stop(): void;
  pause(): void;
  resume(): void;
}
interface RusticLoadedAudio {
  load(): RusticLoadedAudio;
  play(options?: RusticAudioOptions): RusticAudioVoice;
  stop(): void;
  pause(): void;
  resume(): void;
  unload(): void;
  Unload(): void;
}
interface RusticObjectPath extends RusticCaptureApi {
  GetAttribute(name: string): unknown;
  get_attribute(name: string): unknown;
}
interface RusticApi {
  print(...values: unknown[]): void;
  warn(...values: unknown[]): void;
  get_attribute(name: string): unknown;
  GetAttribute(name: string): unknown;
  edit_attribute(name: string, value: unknown): void;
  EditAttribute(name: string, value: unknown): void;
  key(name: string): RusticActionState;
  key_events(): ReadonlyArray<{ key: string; state: string; repeat: boolean }>;
  any_key_pressed(): boolean;
  find_entity(source: string): string | null;
  get_object_attribute(source: string, name: string): unknown;
}
interface RusticOperationHandle {
  id: string;
  state(): { status: string; error?: string | null } | undefined;
  onFinished(callback: () => void): RusticOperationHandle;
  pause(): RusticOperationHandle;
  resume(): RusticOperationHandle;
  cancel(): RusticOperationHandle;
  reverse(): RusticOperationHandle;
}
declare const Audio: {
  load(source: string): RusticLoadedAudio;
  play(source: string, options?: RusticAudioOptions): RusticAudioVoice;
  playAt(source: string, position: [number, number, number], options?: RusticAudioOptions): RusticAudioVoice;
  get_output_devices(): { id: string; name: string }[];
  get_input_devices(): { id: string; name: string }[];
  set_output_device(id?: string | null): void;
  set_input_device(id?: string | null): void;
  volume(voice: RusticAudioVoice, value: number): void;
  pitch(voice: RusticAudioVoice, value: number): void;
  fadeIn(voice: RusticAudioVoice, duration: number, easing?: string, volume?: number): RusticOperationHandle;
  fadeOut(voice: RusticAudioVoice, duration: number, easing?: string): RusticOperationHandle;
  crossfade(oldVoice: RusticAudioVoice, newVoice: RusticAudioVoice, duration: number, easing?: string): RusticOperationHandle;
};
