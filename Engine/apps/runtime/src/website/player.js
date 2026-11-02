"use strict";
const canvas = document.getElementById("game");
const status = document.getElementById("status");
const context = canvas.getContext("2d");
fetch("game-info.json").then(r => r.json()).then(info => { document.title = info.name; }).catch(() => {});
const keys = new Set();
const pointers = [];
let reset = false;
let audio;
let nextAudioTime = 0;
let running = true;
function fail(error) {
  running = false;
  status.textContent = `Game could not run: ${error?.message || String(error)}`;
  status.hidden = false;
  socket?.close();
}
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
function clearInput() { keys.clear(); heldPointers.clear(); pointers.length = 0; reset = true; }
window.addEventListener("blur", clearInput);
document.addEventListener("visibilitychange", () => { clearInput(); });
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
    if (pointers.length >= 1024) { clearInput(); return; }
    pointers.push({ id: String(event.pointerId), device: event.pointerType === "touch" ? "touch" : "mouse", phase, position: [x, y], pressure: event.pressure || 1, aspect: 16 / 9 });
    if (phase === "end" || phase === "cancel") heldPointers.delete(event.pointerId);
    event.preventDefault();
  });
}

const url = new URL("play", location.href);
url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
const socket = new WebSocket(url);
socket.binaryType = "arraybuffer";
let waiting = false;
function sendInput() {
  if (!running || waiting || socket.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({ keys: [...keys], pointers: pointers.splice(0, 1024), reset }));
  reset = false;
  waiting = true;
}
socket.addEventListener("open", () => { canvas.focus(); sendInput(); });
socket.addEventListener("error", () => fail(new Error("Could not connect to the game host.")));
socket.addEventListener("close", () => { if (running) fail(new Error("Connection closed. Reload to start a new game.")); });
socket.addEventListener("message", async event => {
  if (!running) return;
  try {
    if (typeof event.data === "string") throw new Error(JSON.parse(event.data).error || "Unexpected game response");
    const bytes = new Uint8Array(event.data);
    if (bytes.length < 16) throw new Error("Incomplete game frame");
    const header = new DataView(event.data);
    const width = header.getUint32(0, true), height = header.getUint32(4, true);
    const pixels = header.getUint32(8, true), samples = header.getUint32(12, true);
    if (width !== 640 || height !== 360 || samples > 48000 || pixels > 2000000 || bytes.length !== 16 + pixels + samples * 4) throw new Error("Invalid game frame");
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    const bitmap = await createImageBitmap(new Blob([bytes.subarray(16, 16 + pixels)], { type: "image/png" }));
    if (running) context.drawImage(bitmap, 0, 0, width, height);
    bitmap.close();
    playAudio(bytes.subarray(16 + pixels));
    waiting = false;
    requestAnimationFrame(sendInput);
  } catch (error) { fail(error); }
});
window.addEventListener("pagehide", () => { running = false; socket.close(); });
