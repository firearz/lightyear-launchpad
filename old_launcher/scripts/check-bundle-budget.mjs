import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const __dirname = dirname(fileURLToPath(import.meta.url));
const MAX_INITIAL_JS_GZIP_KB = 150;
const distDir = resolve(__dirname, '../apps/desktop/dist');
const assetsDir = join(distDir, 'assets');
const indexHtmlPath = join(distDir, 'index.html');

function formatKb(bytes) {
  return Number((bytes / 1024).toFixed(2));
}

try {
  const indexHtml = readFileSync(indexHtmlPath, 'utf8');
  const allAssets = readdirSync(assetsDir);

  // Extract initial scripts referenced in index.html (entry + modulepreload)
  const initialScriptMatches = [
    ...indexHtml.matchAll(/(?:src|href)="\/?assets\/([^"]+\.js)"/g),
  ].map((m) => m[1]);
  const initialSet = new Set(initialScriptMatches);

  let initialRawBytes = 0;
  let initialGzipBytes = 0;
  let totalJsRawBytes = 0;
  let totalJsGzipBytes = 0;
  let totalCssRawBytes = 0;
  let totalCssGzipBytes = 0;

  const chunkReport = [];

  for (const file of allAssets) {
    const fullPath = join(assetsDir, file);
    if (!statSync(fullPath).isFile()) continue;
    const buf = readFileSync(fullPath);
    const gz = gzipSync(buf, { level: 9 });
    const isInitial = initialSet.has(file);

    if (file.endsWith('.js')) {
      totalJsRawBytes += buf.length;
      totalJsGzipBytes += gz.length;
      if (isInitial) {
        initialRawBytes += buf.length;
        initialGzipBytes += gz.length;
      }
    } else if (file.endsWith('.css')) {
      totalCssRawBytes += buf.length;
      totalCssGzipBytes += gz.length;
    }

    chunkReport.push({
      file,
      type: file.endsWith('.js') ? 'js' : file.endsWith('.css') ? 'css' : 'asset',
      initial: isInitial,
      rawKb: formatKb(buf.length),
      gzipKb: formatKb(gz.length),
    });
  }

  // Fallback if index.html didn't list modulepreload chunks: if initialSet is empty, count index-*.js
  if (initialGzipBytes === 0) {
    for (const c of chunkReport) {
      if (c.type === 'js' && c.file.startsWith('index-')) {
        c.initial = true;
        initialRawBytes += Math.round(c.rawKb * 1024);
        initialGzipBytes += Math.round(c.gzipKb * 1024);
      }
    }
  }

  const initialJsGzipKb = formatKb(initialGzipBytes);
  const totalJsGzipKb = formatKb(totalJsGzipBytes);
  const totalCssGzipKb = formatKb(totalCssGzipBytes);
  const passed = initialJsGzipKb <= MAX_INITIAL_JS_GZIP_KB;

  const summary = {
    timestamp: new Date().toISOString(),
    budgetGzipKb: MAX_INITIAL_JS_GZIP_KB,
    initialJsRawKb: formatKb(initialRawBytes),
    initialJsGzipKb,
    totalAllChunksJsRawKb: formatKb(totalJsRawBytes),
    totalAllChunksJsGzipKb: totalJsGzipKb,
    totalCssRawKb: formatKb(totalCssRawBytes),
    totalCssGzipKb,
    passed,
    chunks: chunkReport,
  };

  writeFileSync(
    join(distDir, 'bundle-budget-report.json'),
    JSON.stringify(summary, null, 2) + '\n',
    'utf8'
  );

  console.log('=== Kestryx Frontend Bundle Budget Report ===');
  console.log(`Initial JS (gzip):     ${initialJsGzipKb} KB / ${MAX_INITIAL_JS_GZIP_KB} KB budget`);
  console.log(`All JS Chunks (gzip):  ${totalJsGzipKb} KB`);
  console.log(`Total CSS (gzip):      ${totalCssGzipKb} KB`);
  console.log(`Status:                ${passed ? 'PASS' : 'FAIL'}`);
  for (const c of chunkReport) {
    console.log(
      `  - ${c.file.padEnd(36)} [${c.initial ? 'INITIAL' : 'LAZY   '}] raw=${String(c.rawKb).padStart(6)} KB | gzip=${String(c.gzipKb).padStart(6)} KB`
    );
  }

  if (!passed) {
    console.error(
      `ERROR: Initial JS gzip size (${initialJsGzipKb} KB) exceeds ${MAX_INITIAL_JS_GZIP_KB} KB budget!`
    );
    process.exit(1);
  }
} catch (err) {
  console.error('Failed to check bundle budget:', err.message);
  process.exit(1);
}
