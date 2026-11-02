import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { after, before, test } from "node:test";
import { chromium } from "playwright";

const source = new URL("../../apps/runtime/src/website/", import.meta.url);
let browser, png;
before(async () => {
  browser = await chromium.launch({ executablePath: process.env.RUSTIC_CHROMIUM || undefined, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
  const page = await browser.newPage();
  png = Buffer.from(await page.evaluate(() => {
    const canvas = document.createElement("canvas");
    canvas.width = 640; canvas.height = 360;
    const context = canvas.getContext("2d");
    context.fillStyle = "red"; context.fillRect(0, 0, 640, 360);
    return [...Uint8Array.from(atob(canvas.toDataURL().split(",")[1]), c => c.charCodeAt(0))];
  }));
  await page.close();
});
after(async () => { await browser?.close(); });

async function openPlayer(t, { error, invalidFrame = false } = {}) {
  const context = await browser.newContext({ hasTouch: true });
  t.after(() => context.close());
  const page = await context.newPage();
  const inputs = [], errors = [];
  page.on("pageerror", error => errors.push(error.message));
  t.after(() => assert.deepEqual(errors, []));
  await page.route("http://game.test/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/" || path === "/player.js") {
      await route.fulfill({ contentType: path === "/" ? "text/html" : "text/javascript", body: await readFile(new URL(path === "/" ? "player.html" : "player.js", source)) });
    } else if (path === "/game-info.json") await route.fulfill({ json: { name: "Hosted Game" } });
    else await route.fulfill({ status: 404 });
  });
  await page.routeWebSocket("ws://game.test/play", socket => {
    socket.onMessage(message => {
      inputs.push(JSON.parse(message));
      if (error) { socket.send(JSON.stringify({ error })); return; }
      if (invalidFrame) { socket.send(Buffer.alloc(8)); return; }
      const frame = Buffer.alloc(16 + png.length);
      frame.writeUInt32LE(640, 0); frame.writeUInt32LE(360, 4);
      frame.writeUInt32LE(png.length, 8); frame.writeUInt32LE(0, 12);
      png.copy(frame, 16);
      socket.send(frame);
    });
  });
  await page.goto("http://game.test/");
  return { page, inputs };
}

test("hosted game renders PNG frames, forwards input and starts audio on a gesture", async t => {
  const { page, inputs } = await openPlayer(t);
  await page.waitForFunction(() => document.querySelector("canvas").getContext("2d").getImageData(0, 0, 1, 1).data[0] === 255);
  assert.equal(await page.title(), "Hosted Game");
  assert.equal(await page.locator("#status").isVisible(), false);
  const canvas = page.locator("#game");
  await canvas.focus();
  await page.keyboard.down("KeyD");
  await page.waitForTimeout(100);
  await page.keyboard.up("KeyD");
  assert.ok(inputs.some(input => input.keys.includes("KeyD")));
  await page.touchscreen.tap(300, 200);
  await page.waitForTimeout(100);
  assert.ok(inputs.some(input => input.pointers.some(pointer => pointer.device === "touch" && pointer.phase === "start")));
  await page.waitForFunction(() => audio?.state === "running");
  await page.evaluate(() => window.dispatchEvent(new Event("blur")));
  await page.waitForTimeout(100);
  assert.ok(inputs.some(input => input.reset && input.keys.length === 0));
});

test("native startup failures appear in the player", async t => {
  const { page } = await openPlayer(t, { error: "Python toolchain is unavailable" });
  await page.locator("#status").waitFor({ state: "visible" });
  assert.match(await page.locator("#status").textContent(), /Python toolchain is unavailable/);
});

test("truncated transport frames fail visibly", async t => {
  const { page } = await openPlayer(t, { invalidFrame: true });
  await page.locator("#status").waitFor({ state: "visible" });
  assert.match(await page.locator("#status").textContent(), /Incomplete game frame/);
});
