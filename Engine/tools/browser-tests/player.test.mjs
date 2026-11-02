// Exercise the shipped loader in a browser without requiring a compiled game runtime.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { after, before, test } from "node:test";
import { chromium } from "playwright";

const source = new URL("../../apps/editor/src/project_build/", import.meta.url);
let browser;
before(async () => {
  browser = await chromium.launch({ executablePath: process.env.RUSTIC_CHROMIUM || undefined, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
});
after(async () => { await browser?.close(); });

async function openPlayer(t, { missingRuntime = false, badManifest = false } = {}) {
  const context = await browser.newContext({ hasTouch: true });
  t.after(() => context.close());
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  t.after(() => assert.deepEqual(errors, []));
  await page.route("http://game.test/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/" || path === "/player.js") {
      await route.fulfill({
        contentType: path === "/" ? "text/html" : "text/javascript",
        body: await readFile(new URL(path === "/" ? "player.html" : "player.js", source)),
      });
    } else if (path === "/game-info.json") {
      await route.fulfill({ json: { name: "Test Game" } });
    } else if (path === "/game-files.json" && !badManifest) {
      await route.fulfill({ json: [] });
    } else if (path === "/rustic-browser.js" && !missingRuntime) {
      await route.fulfill({ contentType: "text/javascript", body: `
        const files = new Map();
        Module.FS = {
          stat(path) { if (!files.has(path)) throw new Error('Missing file'); },
          readFile(path) { return files.get(path); },
          writeFile(path, bytes) { files.set(path, bytes); },
        };
        Module._rustic_tick = () => {
          files.set('/frame.json', '[1,1]');
          files.set('/frame.rgba', new Uint8Array([255,0,0,255]));
        };
        Module.addRunDependency = () => {};
        Module.removeRunDependency = () => Module.onRuntimeInitialized();
        Module.preRun.forEach(callback => callback());
      ` });
    } else {
      await route.fulfill({ status: 404, body: "Not found" });
    }
  });
  await page.goto("http://game.test/");
  return page;
}

test("loads and renders automatically without a player menu or loading overlay", async t => {
  const page = await openPlayer(t);
  await page.waitForFunction(() => document.getElementById("game").dataset.frame);
  assert.equal(await page.getByRole("button").count(), 0);
  assert.equal(await page.locator("#status").isVisible(), false);
  assert.equal(await page.locator("#game").evaluate(canvas => canvas === document.activeElement), true);
  assert.deepEqual(await page.locator("#game").evaluate(canvas => [...canvas.getContext("2d").getImageData(0, 0, 1, 1).data]), [255, 0, 0, 255]);
  assert.equal(await page.evaluate(() => audio), undefined, "Audio should wait for a gesture");
  await page.keyboard.press("Space");
  await page.waitForFunction(() => audio?.state === "running");
  const firstAudio = await page.evaluateHandle(() => audio);
  await page.touchscreen.tap(100, 100);
  assert.equal(await page.evaluate(first => audio === first, firstAudio), true, "Gestures must reuse the audio context");
});

test("a first tap also enables sound without a launch button", async t => {
  const page = await openPlayer(t);
  await page.waitForFunction(() => document.getElementById("game").dataset.frame);
  await page.touchscreen.tap(100, 100);
  await page.waitForFunction(() => audio?.state === "running");
});

test("a missing runtime displays an actionable error automatically", async t => {
  const page = await openPlayer(t, { missingRuntime: true });
  await page.waitForFunction(() => !document.getElementById("status").hidden);
  assert.match(await page.locator("#status").textContent(), /Browser runtime is missing/);
});

test("a missing game manifest displays an error instead of starting gameplay", async t => {
  const page = await openPlayer(t, { badManifest: true });
  await page.waitForFunction(() => !document.getElementById("status").hidden);
  assert.match(await page.locator("#status").textContent(), /Game file manifest is missing/);
  assert.equal(await page.locator("#game").getAttribute("data-frame"), null);
});
