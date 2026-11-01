import { mkdir, copyFile, mkdtemp } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { resolve, join } from "node:path";

await mkdir(".test-data", { recursive: true });
const output = await mkdtemp(resolve(".test-data", "icons-"));
const result = spawnSync(
  process.execPath,
  [
    "node_modules/@tauri-apps/cli/tauri.js",
    "icon",
    "Edge-Lock-Icon.png",
    "--output",
    output,
  ],
  { stdio: "inherit" },
);
if (result.status !== 0) process.exit(result.status ?? 1);
await mkdir("src-tauri/icons", { recursive: true });
await mkdir("public", { recursive: true });
for (const name of ["icon.ico", "32x32.png", "128x128.png", "128x128@2x.png"])
  await copyFile(join(output, name), join("src-tauri/icons", name));
await copyFile("Edge-Lock-Icon.png", "public/edge-lock-icon.png");
console.log("Icônes Windows régénérées depuis Edge-Lock-Icon.png.");
