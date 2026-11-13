// Exercise actual exported WASM/WebGL effects. No mocked runtime or canvas frames.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const workspace = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const temporary = await mkdtemp(join(tmpdir(), "rustic-web-effects-"));
const site = join(temporary, "website");
let browser;
let server;
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
try {
  const fixture = spawnSync("cargo", ["test", "-p", "rustic-editor", "--profile", process.env.RUSTIC_BUILD_PROFILE || "dev-fast", "write_browser_effects_fixture", "--", "--ignored"], {
    cwd: workspace, stdio: "inherit", env: { ...process.env, CARGO_INCREMENTAL: "0", RUSTIC_BROWSER_EFFECTS_DIR: temporary },
  });
  assert.equal(fixture.status, 0, "Could not export the real effects fixture");
  const sources = new Map();
  for (const relative of ["scene/effects.scene", "assets/effects/impact.effect", "assets/effects/impact.effect.rmeta"]) {
    sources.set(relative, await readFile(join(site, "data", relative)));
  }
  const mime = { ".js": "text/javascript", ".html": "text/html", ".json": "application/json", ".wasm": "application/wasm" };
  server = createServer(async (request, response) => {
    try {
      const path = new URL(request.url, "http://localhost").pathname;
      const file = resolve(site, `.${decodeURIComponent(path === "/" ? "/index.html" : path)}`);
      if (!file.startsWith(site + sep)) throw new Error("Invalid path");
      response.setHeader("Content-Type", mime[extname(file)] || "application/octet-stream");
      response.setHeader("Cache-Control", "no-store");
      response.end(await readFile(file));
    } catch { response.writeHead(404); response.end("Not found"); }
  });
  await new Promise(ready => server.listen(0, "127.0.0.1", ready));
  const origin = `http://127.0.0.1:${server.address().port}`;
  browser = await chromium.launch({ executablePath: process.env.RUSTIC_CHROMIUM || undefined, headless: true, args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--disable-dev-shm-usage", "--no-sandbox"] });
  const context = await browser.newContext({ viewport: { width: 640, height: 360 } });
  const page = await context.newPage();
  const logs = [];
  const errors = [];
  page.on("console", message => logs.push(message.text()));
  page.on("pageerror", error => errors.push(error.message));
  const waitLog = async marker => {
    const deadline = Date.now() + 60000;
    while (!logs.some(line => line.includes(marker))) {
      const status = await page.locator("#status").textContent();
      assert.ok(!status.startsWith("Game could"), `${status}\n${logs.join("\n")}`);
      assert.ok(Date.now() < deadline, `Missing ${marker}\n${logs.join("\n")}`);
      await new Promise(done => setTimeout(done, 50));
    }
  };
  await page.goto(origin);
  await waitLog("vfx-ready:rate=45:asset-loaded");
  await waitLog("vfx-warm");
  await page.waitForFunction(() => document.getElementById("game").dataset.frame, null, { timeout: 60000 });
  assert.equal(await page.locator("#status").isVisible(), false);
  assert.match(await page.locator("#gpu").evaluate(canvas => canvas.getContext("webgl2")?.getParameter(WebGL2RenderingContext.VERSION) || ""), /WebGL 2/i, "Effects did not use a real WebGL 2 context");
  const pixels = async () => page.locator("#game").evaluate(canvas => {
    const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    let orange = 0, violet = 0, cyan = 0, blue = 0;
    for (let i = 0; i < data.length; i += 4) {
      const [r, g, b] = [data[i], data[i + 1], data[i + 2]];
      if (r > 70 && r > g * 1.4 && r > b * 1.7) orange++;
      if (r > 50 && b > 70 && r > g * 1.3 && b > g * 1.5) violet++;
      if (g > 60 && b > 70 && g > r * 1.4 && b > r * 1.5) cyan++;
      blue += b;
    }
    return { orange, violet, cyan, meanBlue: blue / (data.length / 4), image: canvas.toDataURL() };
  });
  const baseline = await pixels();
  assert.ok(baseline.orange > 12, `Orange particle pixels missing: ${JSON.stringify({ ...baseline, image: undefined })}`);
  assert.ok(baseline.violet > 12, "Violet beam pixels missing");
  assert.ok(baseline.cyan > 12, "Moving cyan trail pixels missing");
  await page.locator("#game").focus();
  await page.keyboard.down("ArrowDown");
  await waitLog("vfx-paused");
  await page.keyboard.up("ArrowDown");
  const pausedStamp = await page.locator("#game").getAttribute("data-frame");
  await page.waitForFunction(stamp => Number(document.getElementById("game").dataset.frame) > Number(stamp) + 100, pausedStamp, { timeout: 60000 });
  const paused = await pixels();
  const stableStamp = await page.locator("#game").getAttribute("data-frame");
  await page.waitForFunction(stamp => Number(document.getElementById("game").dataset.frame) > Number(stamp) + 150, stableStamp, { timeout: 60000 });
  assert.equal((await pixels()).image, paused.image, "Paused effect rendering changed despite frozen simulation");
  await page.keyboard.down("ArrowDown");
  await waitLog("vfx-resumed");
  await page.keyboard.up("ArrowDown");
  await page.waitForFunction(image => document.getElementById("game").toDataURL() !== image, paused.image, { timeout: 60000 });
  await page.keyboard.down("ArrowUp");
  await waitLog("vfx-replayed:rate=45");
  await page.keyboard.up("ArrowUp");
  await page.waitForFunction(reference => {
    const canvas = document.getElementById("game");
    const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    let blue = 0;
    for (let i = 2; i < data.length; i += 4) blue += data[i];
    return blue / (data.length / 4) > reference + 15;
  }, baseline.meanBlue, { timeout: 60000 });
  for (const [relative, original] of sources) {
    const runtime = await page.evaluate(path => Array.from(Module.FS.readFile(`/game/${path}`)), relative);
    assert.equal(hash(Buffer.from(runtime)), hash(original), `Runtime changed saved ${relative}`);
    assert.equal(hash(await readFile(join(site, "data", relative))), hash(original));
  }
  assert.deepEqual(errors, []);
  if (process.env.RUSTIC_BROWSER_EFFECTS_ARTIFACT_DIR) {
    await mkdir(process.env.RUSTIC_BROWSER_EFFECTS_ARTIFACT_DIR, { recursive: true });
    await page.screenshot({ path: join(process.env.RUSTIC_BROWSER_EFFECTS_ARTIFACT_DIR, "browser-effects.png") });
    await writeFile(join(process.env.RUSTIC_BROWSER_EFFECTS_ARTIFACT_DIR, "browser-effects.json"), JSON.stringify({ baseline: { ...baseline, image: undefined }, webgl: true, checks: ["particles", "beam", "trail", "pause/resume", "replay overlay", "asset persistence"] }, null, 2));
  }
  const source = join(site, "data/assets/effects/impact.effect");
  await writeFile(source, "corrupted effect asset");
  const corrupt = await context.newPage();
  await corrupt.goto(origin);
  await corrupt.waitForFunction(() => document.getElementById("status").textContent.startsWith("Game could"), null, { timeout: 60000 });
  assert.match(await corrupt.locator("#status").textContent(), /integrity/i, "Tampered effect asset was not rejected");
  await writeFile(source, sources.get("assets/effects/impact.effect"));
  console.log("PASS: real exported WebGL particles/trail/beam, pause/resume, replay overlay, saved effect/scene persistence and asset integrity");
} finally {
  if (browser) await browser.close();
  if (server) await new Promise(done => server.close(done));
  await rm(temporary, { recursive: true, force: true });
}
