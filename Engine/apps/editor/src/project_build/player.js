/* The exported game runs locally in WebAssembly; only static files are fetched. */
"use strict";
const canvas = document.getElementById("game");
const status = document.getElementById("status");
fetch("game-info.json").then(response => response.json()).then(info => { document.title = info.name; }).catch(() => {});
const keys = new Set();
const pointers = [];
let reset = false;
let running = false;
let previous = 0;
let context;
let audio;
let nextAudioTime = 0;
function enableAudio() {
  try {
    audio ??= new AudioContext();
    if (audio.state === "suspended") audio.resume().catch(() => {});
  } catch { /* Graphics and gameplay remain available without an audio device. */ }
}
// Browsers require a player gesture for sound, but gameplay starts immediately.
canvas.addEventListener("pointerdown", enableAudio);
canvas.addEventListener("keydown", enableAudio);
function playAudio(bytes) {
  if (!audio || audio.state !== "running" || bytes.length < 8) return;
  if (nextAudioTime > audio.currentTime + 0.3) return;
  const samples = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = Math.floor(bytes.length / 8);
  const buffer = audio.createBuffer(2, count, 48000);
  for (let channel = 0; channel < 2; channel++) {
    const destination = buffer.getChannelData(channel);
    for (let i = 0; i < count; i++) destination[i] = samples.getFloat32((i * 2 + channel) * 4, true);
  }
  const source = audio.createBufferSource();
  source.buffer = buffer;
  source.connect(audio.destination);
  const when = Math.max(audio.currentTime + 0.02, nextAudioTime);
  source.start(when);
  nextAudioTime = when + buffer.duration;
}
function exists(path) {
  try { Module.FS.stat(path); return true; } catch { return false; }
}
function fail(error) {
  running = false;
  status.textContent = `Game could not run: ${error?.message || String(error)}`;
  status.hidden = false;
  console.error(error);
}
function clearInput() { keys.clear(); heldPointers.clear(); pointers.length = 0; reset = true; }
window.addEventListener("blur", clearInput);
document.addEventListener("visibilitychange", () => { clearInput(); previous = 0; });
canvas.addEventListener("keydown", event => {
  event.preventDefault();
  if (event.code) keys.add(event.code);
  keys.add(event.key === " " ? "Space" : event.key.length === 1 ? event.key.toUpperCase() : event.key);
});
canvas.addEventListener("keyup", event => {
  event.preventDefault();
  if (event.code) keys.delete(event.code);
  keys.delete(event.key === " " ? "Space" : event.key.length === 1 ? event.key.toUpperCase() : event.key);
});
const heldPointers = new Set();
for (const [type, phase] of [["pointerdown", "start"], ["pointermove", "move"], ["pointerup", "end"], ["pointercancel", "cancel"]]) {
  canvas.addEventListener(type, event => {
    if (event.pointerType === "mouse" && event.button !== 0 && phase !== "move") return;
    if (phase === "start") { canvas.focus(); canvas.setPointerCapture(event.pointerId); heldPointers.add(event.pointerId); }
    if (!heldPointers.has(event.pointerId)) return;
    const rect = canvas.getBoundingClientRect();
    // CSS object-fit letterboxes the fixed 16:9 game image inside the canvas box.
    const width = Math.min(rect.width, rect.height * 16 / 9);
    const height = width * 9 / 16;
    const x = (event.clientX - rect.left - (rect.width - width) / 2) / width;
    const y = (event.clientY - rect.top - (rect.height - height) / 2) / height;
    if (phase === "start" && (x < 0 || x > 1 || y < 0 || y > 1)) { heldPointers.delete(event.pointerId); return; }
    pointers.push({ id: String(event.pointerId), device: event.pointerType === "touch" ? "touch" : "mouse", phase, position: [x, y], pressure: event.pressure || 1, aspect: 16 / 9 });
    if (phase === "end" || phase === "cancel") heldPointers.delete(event.pointerId);
    event.preventDefault();
  });
}
function frame(timestamp) {
  if (!running) return;
  if (document.hidden) { previous = 0; requestAnimationFrame(frame); return; }
  try {
    const delta = previous ? Math.min((timestamp - previous) / 1000, 0.25) : 0;
    previous = timestamp;
    Module.FS.writeFile("/input.json", JSON.stringify({ keys: [...keys], pointers: pointers.splice(0, 1024), reset }));
    reset = false;
    const width = Math.max(1, Math.min(1280, Math.round(window.innerWidth)));
    const height = Math.max(1, Math.round(width * 9 / 16));
    Module._rustic_tick(width, height, delta);
    if (exists("/error.txt")) throw new Error(Module.FS.readFile("/error.txt", { encoding: "utf8" }));
    if (!exists("/frame.json")) { requestAnimationFrame(frame); return; }
    const [w, h] = JSON.parse(Module.FS.readFile("/frame.json", { encoding: "utf8" }));
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
    const pixels = Module.FS.readFile("/frame.rgba");
    context.putImageData(new ImageData(new Uint8ClampedArray(pixels), w, h), 0, 0);
    if (exists("/audio.f32")) playAudio(Module.FS.readFile("/audio.f32"));
    canvas.dataset.frame = String(timestamp);
    requestAnimationFrame(frame);
  } catch (error) { fail(error); }
}
var Module = {
  locateFile: (path, prefix) => path.endsWith(".wasm") ? "rustic-browser.wasm" : prefix + path,
  canvas: document.getElementById("gpu"),
  print: message => console.log(message),
  printErr: message => console.error(message),
  onAbort: fail,
  preRun: [() => {
    Module.addRunDependency("game-data");
    fetch("game-files.json").then(response => {
      if (!response.ok) throw new Error("Game file manifest is missing");
      return response.json();
    }).then(async files => {
      // Bound download concurrency so large projects do not exhaust browser connections.
      let next = 0;
      async function download() {
        while (next < files.length) {
          const path = files[next++];
          if (typeof path !== "string" || path.split("/").some(part => !part || part === "." || part === "..")) throw new Error("Invalid packaged game path");
          const response = await fetch(`data/${path.split("/").map(encodeURIComponent).join("/")}`);
          if (!response.ok) throw new Error(`Missing game file: ${path}`);
          const target = `/game/${path}`;
          Module.FS.mkdirTree(target.slice(0, target.lastIndexOf("/")));
          Module.FS.writeFile(target, new Uint8Array(await response.arrayBuffer()));
        }
      }
      await Promise.all(Array.from({ length: Math.min(8, files.length) }, download));
      Module.removeRunDependency("game-data");
    }).catch(fail);
  }],
  onRuntimeInitialized: () => {
    // main() loads and verifies the snapshot after runtime initialization callbacks.
    setTimeout(() => {
      if (exists("/error.txt")) { fail(new Error(Module.FS.readFile("/error.txt", { encoding: "utf8" }))); return; }
      if (status.textContent) return; // An earlier initialization failure remains visible.
      running = true;
      canvas.focus();
      requestAnimationFrame(frame);
    }, 0);
  },
};
function startGame() {
  context = canvas.getContext("2d");
  if (!context) { fail(new Error("Canvas rendering is unavailable")); return; }
  canvas.focus();
  const script = document.createElement("script");
  script.src = "rustic-browser.js";
  script.onerror = () => fail(new Error("Browser runtime is missing"));
  document.body.appendChild(script);
}
startGame();
