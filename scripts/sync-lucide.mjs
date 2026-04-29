import { existsSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ProxyAgent, setGlobalDispatcher } from 'undici';

const proxyUrl = process.env.HTTPS_PROXY || process.env.https_proxy || process.env.HTTP_PROXY || process.env.http_proxy;
if (proxyUrl) {
  setGlobalDispatcher(new ProxyAgent(proxyUrl));
  console.log(`Using proxy: ${proxyUrl}\n`);
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const iconsDir = resolve(__dirname, '..', 'cinny', 'src', 'app', 'icons');

const ICONS = [
  'panel-left',
  'user',
  'search',
  'ellipsis-vertical',
  'pin',
  'plus',
  'layout-grid',
  'check-check',
  'reply',
  'message-square-plus',
  'message-square-text',
  'pencil',
  'link',
  'trash-2',
  'smile',
  'smile-plus',
  'circle-alert',
  'mic',
  'sticker',
  'send-horizontal',
  'upload',
  'user-key',
  'user-plus',
  'user-minus',
  'user-check',
  'user-x',
  'user-pen',
  'paperclip',
  'case-sensitive',
  'lock',
  'globe',
  'shield',
  'code',
  'file',
  'file-image',
  'file-text',
  'file-play',
  'file-question-mark',
  'film',
  'audio-lines',
  'mail',
  'settings',
];

function kebabToPascal(str) {
  return str
    .split('-')
    .map((s) => s.charAt(0).toUpperCase() + s.slice(1))
    .join('');
}

function kebabToCamel(str) {
  return str.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
}

function svgAttrToJsx(attrStr) {
  const m = attrStr.match(/^([a-zA-Z][a-zA-Z0-9:.-]*)(="([^"]*)")?$/);
  if (!m) return attrStr;
  const [, name, full, value] = m;
  const jsxName = kebabToCamel(name);
  return value !== undefined ? `${jsxName}="${value}"` : jsxName;
}

function convertSvgToInnerJsx(svg) {
  const innerMatch = svg.match(/<svg[^>]*>([\s\S]*?)<\/svg>/);
  if (!innerMatch) throw new Error('Could not parse SVG');

  let inner = innerMatch[1].trim();

  // Remove attributes now handled by the wrapping <g>
  const removeAttrs = [
    'stroke="currentColor"',
    'stroke-width="2"',
    'stroke-linecap="round"',
    'stroke-linejoin="round"',
    'fill="none"',
    /xmlns="[^"]*"/,
  ];
  for (const attr of removeAttrs) {
    inner = inner.replace(new RegExp(`\\s+${typeof attr === 'string' ? attr.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') : attr.source}`, 'g'), '');
  }

  // Convert SVG attributes to JSX camelCase on inner elements
  inner = inner.replace(/<(\w+)(\s[^>]*?)\s*\/?>/g, (match, tag, attrs) => {
    const converted = attrs.replace(/([a-zA-Z][a-zA-Z0-9:.-]*(?:="[^"]*")?)/g, (a) =>
      svgAttrToJsx(a.trim()),
    );
    // Re-detect self-closing
    const selfClose = match.endsWith('/>') ? '/>' : '>';
    return `<${tag}${converted}${selfClose}`;
  });

  return inner;
}

function generateTsx(iconName, innerJsx) {
  const pascalName = kebabToPascal(iconName);
  const pad = (n) => ' '.repeat(n);
  const indented = innerJsx
    .split('\n')
    .map((line) => (line ? pad(6) + line : line))
    .join('\n');

  return `import React from 'react';

export function ${pascalName}Icon() {
  // https://lucide.dev/icons/${iconName}
  return (
    <g
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
${indented}
    </g>
  );
}
`;
}

async function fetchSvg(iconName) {
  const url = `https://raw.githubusercontent.com/lucide-icons/lucide/refs/heads/main/icons/${iconName}.svg`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.text();
}

async function main() {
  let synced = 0;
  let skipped = 0;
  let failed = 0;
  const failures = [];

  console.log('Syncing Lucide icons...\n');

  for (const iconName of ICONS) {
    const pascalName = kebabToPascal(iconName);
    const fileName = `${pascalName}Icon.tsx`;
    const filePath = resolve(iconsDir, fileName);

    if (existsSync(filePath)) {
      console.log(`  SKIP  ${fileName} (already exists)`);
      skipped++;
      continue;
    }

    try {
      const svg = await fetchSvg(iconName);
      const innerJsx = convertSvgToInnerJsx(svg);
      const tsx = generateTsx(iconName, innerJsx);
      writeFileSync(filePath, tsx, 'utf-8');
      console.log(`  SYNC  ${fileName}`);
      synced++;
    } catch (err) {
      console.error(`  FAIL  ${fileName}: ${err.message}`);
      failed++;
      failures.push({ iconName, error: err.message });
    }
  }

  console.log(`\nSummary: ${synced} synced, ${skipped} skipped, ${failed} failed`);
  if (failures.length) {
    console.log('Failures:');
    failures.forEach((f) => console.log(`  - ${f.iconName}: ${f.error}`));
    process.exit(1);
  }
}

main();
