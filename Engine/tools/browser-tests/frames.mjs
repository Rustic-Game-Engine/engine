// The player changes its marker only after presenting a completed frame.
export async function waitForRenderedFrames(page, count, timeout = 60000) {
  await page.waitForFunction(({ initial, count }) => {
    const frame = Number(document.getElementById("game").dataset.frame);
    return frame >= initial + count;
  }, {
    initial: Number(await page.locator("#game").getAttribute("data-frame")),
    count,
  }, { timeout });
}
