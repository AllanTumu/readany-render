import { expect, test } from "@playwright/test";
test("the real wasm build paints non-white canvas pixels without a long task", async ({ page }) => {
  await page.goto("http://127.0.0.1:4199/browser/test.html");
  await page.waitForFunction(() => document.body.dataset.done === "true");
  expect(await page.locator("body").getAttribute("data-error")).toBeNull();
  expect(await page.locator("body").getAttribute("data-format")).toBe("xlsx");
  expect(await page.locator("body").getAttribute("data-pages")).toBe("1");
  expect(Number(await page.locator("body").getAttribute("data-dark"))).toBeGreaterThan(0);
  expect(Number(await page.locator("body").getAttribute("data-long-task"))).toBeLessThanOrEqual(50);
});

test("a wide sheet stays in wasm while a viewport crosses the boundary", async ({ page }) => {
  await page.goto("http://127.0.0.1:4199/browser/viewport.html");
  await page.waitForFunction(() => document.body.dataset.done === "true", undefined, {
    timeout: 30_000,
  });
  expect(await page.locator("body").getAttribute("data-error")).toBeNull();
  expect(await page.locator("body").getAttribute("data-format")).toBe("xlsx");
  expect(Number(await page.locator("body").getAttribute("data-page-width"))).toBeGreaterThan(25_000);
  expect(Number(await page.locator("body").getAttribute("data-items"))).toBeGreaterThan(0);
  expect(Number(await page.locator("body").getAttribute("data-items"))).toBeLessThan(2_000);
  expect(await page.locator("body").getAttribute("data-canvas")).toBe("600x400");
  expect(Number(await page.locator("body").getAttribute("data-dark"))).toBeGreaterThan(0);
});
