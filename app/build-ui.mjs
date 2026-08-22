// 构建正式前端产物到 dist-ui/
// 关键：mock.js 被物理排除 —— 正式安装包中不存在任何 mock 数据。
import { copyFileSync, cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";

const FILES = ["index.html", "trap.js", "styles.css"]; // 注意：没有 mock.js

rmSync("dist-ui", { recursive: true, force: true });
mkdirSync("dist-ui", { recursive: true });
for (const f of FILES) copyFileSync(`ui/${f}`, `dist-ui/${f}`);
// ES Modules 源码树（mock.js 位于 ui/ 根目录，天然被排除）
cpSync("ui/src", "dist-ui/src", { recursive: true });
copyFileSync("app-icon.svg", "dist-ui/app-icon.svg");
mkdirSync("dist-ui/icons/option-2", { recursive: true });
for (const f of ["service.svg", "folder.svg", "recent.svg"]) {
  copyFileSync(`ui/icons/option-2/${f}`, `dist-ui/icons/option-2/${f}`);
}

// 源文件可直接从 ui/ 预览；正式产物中的图标则与 index.html 同级。
const indexPath = "dist-ui/index.html";
const indexHtml = readFileSync(indexPath, "utf8").replaceAll("../app-icon.svg", "app-icon.svg");
writeFileSync(indexPath, indexHtml);

console.log("dist-ui 已生成（ES Modules 构建，已排除 mock.js 等开发文件）");
