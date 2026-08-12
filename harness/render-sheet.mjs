import { chromium } from "../browser/node_modules/playwright/index.mjs";
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

function fontData(name) {
  return readFileSync(new URL(`../crates/readany-render/fonts/Liberation/${name}`, import.meta.url)).toString("base64");
}

const liberationCss = [
  ["LiberationSans-Regular.ttf", 400, "normal"],
  ["LiberationSans-Bold.ttf", 700, "normal"],
  ["LiberationSans-Italic.ttf", 400, "italic"],
  ["LiberationSans-BoldItalic.ttf", 700, "italic"],
].map(([file, weight, style]) =>
  `@font-face{font-family:'Liberation Sans';src:url(data:font/ttf;base64,${fontData(file)}) format('truetype');font-weight:${weight};font-style:${style}}`
).join("\n");

const [html, output, boxesOutput, scaleText, xText, yText, widthText, heightText] = process.argv.slice(2);
if (!heightText) throw new Error("usage: render-sheet.mjs HTML OUTPUT BOXES SCALE X Y WIDTH HEIGHT");
const scale = Number(scaleText);
const clip = {
  x: Number(xText),
  y: Number(yText),
  width: Number(widthText),
  height: Number(heightText),
};
const browser = await chromium.launch({ headless: true });
try {
  const context = await browser.newContext({
    deviceScaleFactor: scale,
    viewport: {
      width: Math.ceil(clip.x + clip.width),
      height: Math.ceil(clip.y + clip.height),
    },
  });
  const page = await context.newPage();
  await page.goto(pathToFileURL(html).href);
  const adjustedClip = await page.evaluate((requested) => {
    const table = document.querySelector("table");
    if (!table) throw new Error("LibreOffice HTML contains no sheet table");
    const widths = [];
    for (const group of table.querySelectorAll("colgroup")) {
      const width = Number(group.getAttribute("width") ?? 64);
      const span = Number(group.getAttribute("span") ?? 1);
      for (let index = 0; index < span; index++) widths.push(width);
    }
    let columnStart = 0;
    let columnOffset = 0;
    while (
      columnStart < widths.length &&
      columnOffset + widths[columnStart] <= requested.x
    ) {
      columnOffset += widths[columnStart++];
    }
    let columnEnd = columnStart;
    let columnRight = columnOffset;
    while (columnEnd < widths.length && columnRight < requested.x + requested.width) {
      columnRight += widths[columnEnd++];
    }

    const rows = [...table.querySelectorAll("tr")];
    let rowStart = 0;
    let rowOffset = 0;
    const heights = rows.map((row) => Number(row.querySelector("td,th")?.getAttribute("height") ?? 20));
    while (rowStart < rows.length && rowOffset + heights[rowStart] <= requested.y) {
      rowOffset += heights[rowStart++];
    }
    let rowEnd = rowStart;
    let rowBottom = rowOffset;
    while (rowEnd < rows.length && rowBottom < requested.y + requested.height) {
      rowBottom += heights[rowEnd++];
    }
    rows.forEach((row, rowIndex) => {
      if (rowIndex < rowStart || rowIndex >= rowEnd) {
        row.remove();
        return;
      }
      let logicalColumn = 0;
      for (const cell of [...row.children]) {
        const span = Number(cell.getAttribute("colspan") ?? 1);
        const cellEnd = logicalColumn + span;
        cell.dataset.sourceKey = `cell:${rowIndex}:${logicalColumn}`;
        if (cellEnd <= columnStart || logicalColumn >= columnEnd) {
          cell.remove();
        } else {
          const visibleSpan = Math.min(cellEnd, columnEnd) - Math.max(logicalColumn, columnStart);
          if (visibleSpan === 1) cell.removeAttribute("colspan");
          else cell.setAttribute("colspan", String(visibleSpan));
        }
        logicalColumn = cellEnd;
      }
    });
    for (const group of [...table.querySelectorAll("colgroup")]) group.remove();
    const group = document.createElement("colgroup");
    for (const width of widths.slice(columnStart, columnEnd)) {
      const column = document.createElement("col");
      column.style.width = `${width}px`;
      group.append(column);
    }
    table.prepend(group);
    table.style.width = `${widths.slice(columnStart, columnEnd).reduce((sum, width) => sum + width, 0)}px`;
    table.style.tableLayout = "fixed";
    return {
      x: requested.x - columnOffset,
      y: requested.y - rowOffset,
      width: requested.width,
      height: requested.height,
    };
  }, clip);
  await page.addStyleTag({
    content:
      `${liberationCss}\nhtml,body{margin:0!important;padding:0!important} body,div,table,thead,tbody,tfoot,tr,th,td,p{font-size:10pt;font-family:'Liberation Sans'!important} td,th{white-space:nowrap;overflow:hidden}`,
  });
  await page.evaluate(() => document.fonts.ready);
  const boxes = await page.evaluate((visible) => {
    const results = [];
    for (const cell of document.querySelectorAll("td,th")) {
      const walker = document.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
      while (walker.nextNode()) {
        const node = walker.currentNode;
        const text = node.textContent ?? "";
        for (const match of text.matchAll(/\S+/gu)) {
          const range = document.createRange();
          range.setStart(node, match.index);
          range.setEnd(node, match.index + match[0].length);
          const rect = range.getBoundingClientRect();
          if (
            rect.right > visible.x && rect.left < visible.x + visible.width &&
            rect.bottom > visible.y && rect.top < visible.y + visible.height
          ) {
            results.push({
              text: match[0],
              x: rect.left - visible.x,
              y: rect.top - visible.y,
              width: rect.width,
              height: rect.height,
              source_key: cell.dataset.sourceKey,
            });
          }
        }
      }
    }
    return results;
  }, adjustedClip);
  writeFileSync(boxesOutput, `${JSON.stringify(boxes, null, 2)}\n`);
  await page.screenshot({ path: output, clip: adjustedClip });
} finally {
  await browser.close();
}
