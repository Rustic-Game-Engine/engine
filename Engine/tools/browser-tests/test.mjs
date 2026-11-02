import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const workspace = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const temporary = await mkdtemp(join(tmpdir(), "rustic-web-smoke-"));
const site = join(temporary, "website");
let browser;
let server;
try {
  const fixture = spawnSync("cargo", ["test", "-p", "rustic-editor", "--profile", process.env.RUSTIC_BUILD_PROFILE || "dev-fast", "write_browser_smoke_fixture", "--", "--ignored"], {
    cwd: workspace, stdio: "inherit", env: { ...process.env, CARGO_INCREMENTAL: "0", RUSTIC_BROWSER_SMOKE_DIR: temporary },
  });
  assert.equal(fixture.status, 0, "Could not export the shared-runtime fixture");
  const mime = { ".js": "text/javascript", ".html": "text/html", ".json": "application/json", ".wasm": "application/wasm" };
  server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, "http://localhost");
      const file = resolve(site, `.${decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname)}`);
      if (!file.startsWith(site + sep)) throw new Error("Invalid path");
      response.setHeader("Content-Type", mime[extname(file)] || "application/octet-stream");
      response.end(await readFile(file));
    } catch { response.writeHead(404); response.end("Not found"); }
  });
  await new Promise(resolveReady => server.listen(0, "127.0.0.1", resolveReady));
  const origin = `http://127.0.0.1:${server.address().port}`;
  browser = await chromium.launch({ executablePath: process.env.RUSTIC_CHROMIUM || undefined, headless: true, args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--disable-dev-shm-usage", "--no-sandbox"] });
  const context = await browser.newContext({ viewport: { width: 640, height: 360 }, hasTouch: true });
  const page = await context.newPage();
  const logs = [];
  page.on("console", message => logs.push(message.text()));
  page.on("pageerror", error => logs.push(`PAGE ERROR: ${error.message}`));
  await page.goto(origin);
  await page.waitForFunction(() => document.getElementById("game").dataset.frame || document.getElementById("status").textContent.startsWith("Game could"), null, { timeout: 60000 });
  assert.equal(await page.locator("#status").textContent(), "", logs.join("\n"));
  assert.equal(await page.locator("#status").isVisible(), false);
  assert.equal(await page.getByRole("button").count(), 0, "Export added a player UI");
  assert.match(await readFile(join(site, "readme.md"), "utf8"), /python -m http\.server 8080/);
  assert.ok(logs.some(line => line.includes("lua-started")), "Lua startup did not execute");
  assert.ok(logs.some(line => line.includes("javascript-started")), "JavaScript startup did not execute");
  const center = await page.locator("#game").evaluate(canvas => [...canvas.getContext("2d").getImageData(canvas.width / 2, canvas.height / 2, 1, 1).data]);
  assert.ok(center[0] > 100 && center[0] > center[2] * 2, `Expected the red game object, got ${center}`);
  await Promise.all([
    page.waitForEvent("console", { predicate: message => message.text().includes("object-clicked"), timeout: 15000 }),
    page.touchscreen.tap(320, 180),
  ]);
  assert.ok(logs.some(line => line.includes("touch-started")), logs.join("\n"));
  assert.ok(logs.some(line => line.includes("object-clicked")), logs.join("\n"));
  assert.equal(await page.evaluate(() => audio?.state), "running", "Game interaction did not enable audio");
  const before = await page.locator("#game").evaluate(canvas => canvas.toDataURL());
  await Promise.all([
    page.waitForEvent("console", { predicate: message => message.text().includes("key-moved"), timeout: 15000 }),
    page.keyboard.down("KeyD"),
  ]);
  await page.waitForFunction(expected => document.getElementById("game").toDataURL() !== expected, before, { timeout: 15000 });
  await page.keyboard.up("KeyD");
  assert.ok(logs.some(line => line.includes("key-moved")), logs.join("\n"));
  const after = await page.locator("#game").evaluate(canvas => canvas.toDataURL());
  assert.notEqual(after, before, "Game script input did not change the rendered world");
  assert.ok(!logs.some(line => line.startsWith("PAGE ERROR:")), logs.join("\n"));
  await page.close();
  const source = join(site, "data/scripts/controller.js");
  const original = await readFile(source);
  await writeFile(source, "corrupted game script");
  const corrupt = await context.newPage();
  await corrupt.goto(origin);
  await corrupt.waitForFunction(() => document.getElementById("status").textContent.startsWith("Game could"), null, { timeout: 60000 });
  assert.match(await corrupt.locator("#status").textContent(), /integrity/i);
  assert.equal(await corrupt.locator("#status").isVisible(), true);
  await writeFile(source, original);
  console.log("PASS: direct WASM game rendering, Lua/JS, keyboard, touch/object clicks, and snapshot integrity");
} finally {
  if (browser) await browser.close();
  if (server) await new Promise(done => server.close(done));
  await rm(temporary, { recursive: true, force: true });
}
