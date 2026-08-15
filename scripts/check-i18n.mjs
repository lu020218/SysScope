/**
 * 校验 i18n key 的一致性。
 *
 * en.ts 声明为 Record<Keys, string>，所以「en 漏翻译」由 tsc 拦截；
 * 但 HTML 的 data-i18n 与 TS 里的 t("...") 都是普通字符串，类型系统看不见 ——
 * 这里补上那一半：引用了但语言包没有的 key（会显示成 key 本身），
 * 以及语言包里已经没人用的 key（该删）。
 *
 *   node scripts/check-i18n.mjs
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(ts|html)$/.test(p)) out.push(p);
  }
  return out;
}

const files = [
  join(ROOT, "index.html"),
  join(ROOT, "overlay.html"),
  // 跳过 i18n 自身：语言包是 key 的定义方，i18n.ts 的文档注释里也有 t("...") 示例
  ...walk(join(ROOT, "src")).filter(
    (p) => !p.includes("locales") && !p.endsWith("i18n.ts"),
  ),
];

const used = new Map(); // key -> 首次引用位置
for (const file of files) {
  const text = readFileSync(file, "utf8");
  const patterns = [
    /data-i18n(?:-title)?="([^"]+)"/g,
    /\bt\(\s*"([^"]+)"/g,
  ];
  for (const re of patterns) {
    for (const m of text.matchAll(re)) {
      if (!used.has(m[1])) {
        const line = text.slice(0, m.index).split("\n").length;
        used.set(m[1], `${file.slice(ROOT.length)}:${line}`);
      }
    }
  }
}

const zh = readFileSync(join(ROOT, "src/locales/zh-CN.ts"), "utf8");
const defined = new Set([...zh.matchAll(/^\s*"([^"]+)":/gm)].map((m) => m[1]));

const missing = [...used].filter(([k]) => !defined.has(k));
const unused = [...defined].filter((k) => !used.has(k));

for (const [key, where] of missing) {
  console.error(`missing key: "${key}" referenced at ${where}`);
}
for (const key of unused) {
  console.error(`unused key : "${key}" defined in locales but never referenced`);
}

console.log(
  `i18n: ${defined.size} keys defined, ${used.size} referenced, ` +
    `${missing.length} missing, ${unused.length} unused`,
);
process.exit(missing.length || unused.length ? 1 : 0);
