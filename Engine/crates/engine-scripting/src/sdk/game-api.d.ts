// Canonical roots. Named children use the scene and Explorer hierarchy.
interface RusticSceneObject {
  [name: string]: any;
  readonly __source: string;
  EditAttribute(name: string, value: unknown): void;
  edit_attribute(name: string, value: unknown): void;
  tween(property: string, value: unknown, timing: number | { duration?: number; speed?: number; easing?: string }, easing?: string): any;
  moveTo(position: number[], timing: number | { duration?: number; speed?: number; easing?: string }, easing?: string): any;
  destroy(): void;
}
interface RusticExplorerFile {
  [name: string]: any;
  readonly __file_path: string;
  play(options?: { volume?: number; pitch?: number; loop?: boolean }): any;
  add(parent?: string): void;
  clone(parent?: string): void;
  setSkyTexture(): RusticSceneEnvironment;
}
interface RusticGlobalApi extends RusticApi {
  [name: string]: any;
  files: RusticExplorerFile;
  after(delay: number, callback: () => void): any;
  every(interval: number, callback: () => void, count?: number): any;
  lerp(from: number | number[], to: number | number[], progress: number, easing?: string): number | number[];
  getEnvironment(): RusticSceneEnvironment;
  setEnvironment(settings: Partial<RusticSceneEnvironment>): RusticSceneEnvironment;
  setCurrentCamera(object: string | RusticSceneObject): void;
}
interface RusticSceneApi extends RusticSceneObject {
  load(path: string): string;
  unload(scene: string): void;
  switchTo(scene: string): void;
  isLoaded(scene: string): boolean;
  getLoaded(): string[];
  getActive(): string | null;
}
declare const game: Readonly<{ scene: RusticSceneApi; rustic: RusticGlobalApi }>;
