/* 预览模式 mock 后端 —— 仅供浏览器开发预览使用
 * 注意：本文件不会被打包进正式产物（构建脚本物理排除），
 * 正式安装包中不存在任何 mock 数据。
 */
import { sleep } from "./src/util.js";
import { state } from "./src/state.js";
import { showToast } from "./src/toast.js";

const BUILTIN_MOCK = [
  ["notebook","6.4.8"],["pandas","1.3.4"],["numpy","1.22.4"],["scipy","1.7.1"],
  ["matplotlib","3.4.3"],["seaborn","0.11.2"],["openpyxl","3.0.9"],["xlrd","2.0.1"],
  ["Pillow","8.4.0"],["opencv-python","4.10.0.84"],["scikit-learn","0.24.2"],
  ["xgboost","2.1.1"],["imbalanced-learn","0.8.1"],["onnxruntime","1.12.1"],
  ["traitlets","5.1.1"],["matplotlib-inline","0.1.3"],
];

window.__JUPITER_MOCK__ = {
  packages: BUILTIN_MOCK.map(([name, version]) => ({ name, version, source: "builtin", required_by: [] }))
    .concat([
      { name: "requests", version: "2.31.0", source: "explicit", required_by: [] },
      { name: "beautifulsoup4", version: "4.12.2", source: "explicit", required_by: [] },
      { name: "urllib3", version: "2.0.7", source: "dependency", required_by: ["requests"] },
      { name: "certifi", version: "2024.2.2", source: "dependency", required_by: ["requests"] },
      { name: "soupsieve", version: "2.5", source: "dependency", required_by: ["beautifulsoup4"] },
    ]),
  running: null,
  envReady: true,

  async ensureEnv(onProgress) {
    if (this.envReady) return { ready: true };
    for (let p = 8; p <= 100; p += 7) {
      onProgress(p, p < 60 ? "正在解压内置 Python 3.9.7 环境…" : "正在校验环境完整性…");
      await sleep(220);
    }
    this.envReady = true;
    return { ready: true };
  },
  async listPackages() { await sleep(250); return this.packages.slice(); },
  async installPackage(spec, onLog) {
    const lines = [
      [`Looking in indexes: ${state.settings.mirror}`, "dim"],
      [`Collecting ${spec}`, ""],
      [`  Downloading https://mirrors.aliyun.com/pypi/packages/.../${spec.split("==")[0]}-py3-none-any.whl (62 kB)`, "dim"],
      [`Installing collected packages: ${spec.split("==")[0]}`, ""],
    ];
    for (const [t, c] of lines) { onLog(t, c); await sleep(450); }
    await sleep(500);
    onLog(`Successfully installed ${spec}`, "ok");
    const [name, version] = spec.includes("==") ? spec.split("==") : [spec, "latest"];
    if (!this.packages.find(p => p.name === name)) this.packages.push({ name, version, source: "explicit", required_by: [] });
    return { ok: true };
  },
  async uninstallPackage(name) {
    await sleep(300);
    const p = this.packages.find(p => p.name === name);
    if (p && p.source === "builtin") throw "内置包不可卸载（重置环境可恢复出厂状态）";
    if (p && p.required_by && p.required_by.length) {
      throw `无法卸载 ${name}：${p.required_by.join("、")} 依赖它，卸载会导致这些包不可用。如需移除，请先卸载上述包。`;
    }
    this.packages = this.packages.filter(p => p.name !== name);
  },
  async startNotebook(_workdir, _openBrowser) {
    await sleep(1200);
    this.running = { port: 8888, token: "mock", url: "http://127.0.0.1:8888" };
    return this.running;
  },
  async stopNotebook() { await sleep(400); this.running = null; },
  async defaultWorkdir() { return null; },
  async listRecent() { return []; },
  async openNotebookUrl() {
    showToast("预览模式：真实应用中将打开 " + (this.running ? this.running.url : ""), "info");
  },
  async checkUpdates() { await sleep(300); return { latest_version: null, release_notes: null, release_url: null, patches: [] }; },
  async applyPatch(id) { await sleep(500); return "模拟补丁 " + id; },
  async openRecentNotebook(id) { showToast("预览模式：真实应用中将打开最近文件 " + id, "info"); },
  async resetEnv(onStage) {
    const stages = [[12, "正在停止 Notebook 服务…"], [38, "正在清除当前环境…"], [72, "正在恢复出厂环境快照…"], [100, "正在校验环境完整性…"]];
    for (const [p, t] of stages) { onStage(p, t); await sleep(700); }
    this.packages = this.packages.filter(p => p.source === "builtin");
    this.running = null;
  },
};

// 预览模式明显标识（防止假数据被误认为真实功能）
function addPreviewBadge() {
  const badge = document.createElement("div");
  badge.textContent = "预览模式 · 演示数据";
  badge.style.cssText =
    "position:fixed;right:12px;bottom:12px;z-index:10001;background:#F37626;color:#fff;" +
    "font:12px/1 -apple-system,sans-serif;padding:7px 14px;border-radius:999px;" +
    "box-shadow:0 2px 10px rgba(0,0,0,.18);opacity:.92;letter-spacing:.5px";
  document.body.appendChild(badge);
}
// mock.js 是动态注入的，DOMContentLoaded 可能已触发，需判断 readyState
if (document.readyState === "loading") {
  window.addEventListener("DOMContentLoaded", addPreviewBadge);
} else {
  addPreviewBadge();
}
