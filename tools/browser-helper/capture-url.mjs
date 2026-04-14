import { chromium } from "playwright-core";
import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const DEFAULT_BROWSER_CANDIDATES = [
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
  "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
];

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key.startsWith("--")) {
      continue;
    }
    if (value && !value.startsWith("--")) {
      args[key.slice(2)] = value;
      index += 1;
    } else {
      args[key.slice(2)] = "true";
    }
  }
  return args;
}

async function findBrowserExecutable(explicitPath) {
  const candidates = explicitPath
    ? [explicitPath]
    : [process.env.SINGLEPDF_BROWSER_PATH, ...DEFAULT_BROWSER_CANDIDATES].filter(Boolean);

  for (const candidate of candidates) {
    try {
      await fs.access(candidate);
      return candidate;
    } catch {
      // Continue probing.
    }
  }

  throw new Error(
    "No Chromium-based browser executable was found. Pass --browser-executable or set SINGLEPDF_BROWSER_PATH."
  );
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.url) {
    throw new Error("Missing required --url argument.");
  }

  const browserExecutable = await findBrowserExecutable(args["browser-executable"]);
  const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
  const singleFileBootstrap = path.join(rootDir, "SingleFile", "lib", "single-file-bootstrap.js");
  const singleFileCore = path.join(rootDir, "SingleFile", "lib", "single-file.js");

  const browser = await chromium.launch({
    executablePath: browserExecutable,
    headless: true,
    args: [
      "--disable-background-networking",
      "--disable-background-timer-throttling",
      "--disable-renderer-backgrounding",
      "--hide-scrollbars",
      "--mute-audio",
      "--no-first-run",
    ],
  });

  try {
    const context = await browser.newContext({
      viewport: { width: 1440, height: 2200 },
      deviceScaleFactor: 1,
    });
    const page = await context.newPage();
    const timeoutMs = Number(args.timeout ?? "45000");
    page.setDefaultNavigationTimeout(timeoutMs);
    page.setDefaultTimeout(timeoutMs);

    await page.goto(args.url, { waitUntil: "load", timeout: timeoutMs });
    try {
      await page.waitForLoadState("networkidle", { timeout: 5000 });
    } catch {
      // Some pages never reach network idle. Load is enough for alpha capture.
    }
    if (args["post-load-delay-ms"]) {
      await page.waitForTimeout(Number(args["post-load-delay-ms"]));
    }

    await page.addScriptTag({ path: singleFileBootstrap });
    await page.addScriptTag({ path: singleFileCore });

    const snapshot = await page.evaluate(async () => {
      const options = {
        removeHiddenElements: true,
        removeUnusedStyles: true,
        removeUnusedFonts: true,
        removeFrames: false,
        compressHTML: true,
        compressCSS: false,
        loadDeferredImages: true,
        loadDeferredImagesMaxIdleTime: 1500,
        groupDuplicateImages: true,
        blockScripts: true,
        blockVideos: true,
        blockAudios: true,
        saveRawPage: false,
        saveOriginalURLs: true,
        insertMetaCSP: true,
        insertSingleFileComment: true,
        displayStats: false,
        compressContent: false,
      };
      const pageData = await globalThis.singlefile.getPageData(options);
      return {
        metadata: {
          title: pageData?.title || document.title || null,
          url: location.href
        },
        html: pageData?.content || "",
        hints: [],
        capture_mode: "singlefile"
      };
    });

    if (!snapshot || typeof snapshot.html !== "string") {
      throw new Error("SingleFile did not return snapshot HTML content.");
    }

    process.stderr.write(
      `${JSON.stringify({
        title: snapshot.metadata?.title ?? null,
        url: page.url(),
        length: snapshot.html.length,
        hints: snapshot.hints?.length ?? 0,
        capture_mode: snapshot.capture_mode ?? "singlefile"
      })}\n`
    );
    process.stdout.write(JSON.stringify(snapshot));
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack ?? error.message}\n`);
  process.exit(1);
});


