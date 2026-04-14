import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const extensionSourceDir = path.join(rootDir, "extension");
const sourceDir = path.join(rootDir, "SingleFile", "lib");
const args = parseArgs(process.argv.slice(2));
const outputDir = path.resolve(args["out-dir"] ?? path.join(rootDir, "build", "extension"));
const singleFileTargetDir = path.join(outputDir, "lib");
const files = [
  "chrome-browser-polyfill.js",
  "single-file-background.js",
  "single-file-bootstrap.js",
  "single-file-extension-core.js",
  "single-file-extension-frames.js",
  "single-file-frames.js",
  "single-file-hooks-frames.js",
  "single-file.js",
  "web-stream.js"
];

await fs.rm(outputDir, { recursive: true, force: true });
await copyDirectory(extensionSourceDir, outputDir, {
  skip(relativePath) {
    return (
      relativePath === path.join("vendor", "singlefile") ||
      relativePath.startsWith(path.join("vendor", "singlefile") + path.sep)
    );
  }
});

await fs.mkdir(singleFileTargetDir, { recursive: true });
for (const file of files) {
  await fs.copyFile(path.join(sourceDir, file), path.join(singleFileTargetDir, file));
}

console.log(`Prepared extension bundle at ${outputDir}`);
console.log(`Copied ${files.length} SingleFile runtime files into ${singleFileTargetDir}`);

function parseArgs(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 1) {
    const part = argv[index];
    if (!part.startsWith("--")) {
      continue;
    }

    const key = part.slice(2);
    const next = argv[index + 1];
    if (next && !next.startsWith("--")) {
      parsed[key] = next;
      index += 1;
    } else {
      parsed[key] = "true";
    }
  }
  return parsed;
}

async function copyDirectory(sourcePath, targetPath, options) {
  await fs.mkdir(targetPath, { recursive: true });
  const entries = await fs.readdir(sourcePath, { withFileTypes: true });
  for (const entry of entries) {
    const sourceEntry = path.join(sourcePath, entry.name);
    const targetEntry = path.join(targetPath, entry.name);
    const relativePath = path.relative(extensionSourceDir, sourceEntry);
    if (options.skip(relativePath)) {
      continue;
    }

    if (entry.isDirectory()) {
      await copyDirectory(sourceEntry, targetEntry, options);
      continue;
    }

    await fs.copyFile(sourceEntry, targetEntry);
  }
}
