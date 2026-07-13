import { copyFileSync, mkdirSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const src = resolve(root, 'node_modules/@embedpdf/pdfium/dist/pdfium.wasm');
const destDir = resolve(root, 'public');
const dest = resolve(destDir, 'pdfium.wasm');

if (!existsSync(src)) {
  console.error(`pdfium.wasm not found at ${src} — is @embedpdf/pdfium installed?`);
  process.exit(1);
}
mkdirSync(destDir, { recursive: true });
copyFileSync(src, dest);
console.log(`copied ${src} -> ${dest}`);
