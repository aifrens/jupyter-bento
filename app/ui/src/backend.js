/** Tauri 后端抽象层：IPC 调用、调试模式门控、预览 mock 注入、诊断上报 */
import { state } from "./state.js";

/* ================= JSDoc 契约（与 Rust 侧命令返回结构对应） ================= */

/**
 * @typedef {Object} Pkg
 * @property {string} name
 * @property {string} version
 * @property {"builtin"|"explicit"|"dependency"} source
 *   builtin=出厂内置（不可卸载）；explicit=用户显式安装；dependency=随依赖连带安装（受卸载保护）
 * @property {string[]} required_by 当前环境中依赖此包的其他包（「被谁需要」）
 */

/**
 * @typedef {Object} NotebookInfo
 * @property {number} port
 * @property {string} url
 * @property {string} token
 */

/**
 * @typedef {Object} RecentItem
 * @property {string} id
 * @property {string} name
 * @property {number} modified_ms
 */

/* ================= 环境探测与调试模式 ================= */

export const inTauri = !!(window.__TAURI__ && window.__TAURI__.core);

/** 调试模式：浏览器预览必然是开发场景；Tauri 内由 Rust debug_mode 决定 */
export let debugMode = !inTauri;
export function setDebugMode(v) { debugMode = !!v; }

// 浏览器预览：以 ES 动态 import 注入 mock.js（正式构建中该文件不存在，失败即暴露问题）
export const mockReady = inTauri
  ? Promise.resolve()
  : import("../mock.js").then(() => {
      if (!window.__JUPITER_MOCK__) {
        throw new Error("预览组件异常：mock.js 未提供 __JUPITER_MOCK__");
      }
    });

export function mockBackend() {
  const m = window.__JUPITER_MOCK__;
  if (!m) throw new Error("预览后端不可用");
  return m;
}

/* ================= 后端命令封装 ================= */

export const backend = {
  async ensureEnv(onProgress) {
    if (!inTauri) return mockBackend().ensureEnv(onProgress);
    const { listen } = window.__TAURI__.event;
    const un = await listen("setup-progress", e => onProgress(e.payload.percent, e.payload.step));
    try { return await window.__TAURI__.core.invoke("ensure_env"); }
    finally { un(); }
  },

  /** @returns {Promise<Pkg[]>} */
  listPackages() {
    if (!inTauri) return mockBackend().listPackages();
    return window.__TAURI__.core.invoke("list_packages");
  },

  async installPackage(spec, onLog) {
    if (!inTauri) return mockBackend().installPackage(spec, onLog);
    const { listen } = window.__TAURI__.event;
    const un = await listen("pip-log", e => onLog(e.payload.line, e.payload.kind));
    try {
      const [name, version] = spec.includes("==") ? spec.split("==") : [spec, null];
      return await window.__TAURI__.core.invoke("install_package", { name, version, indexUrl: state.settings.mirror });
    } finally { un(); }
  },

  uninstallPackage(name) {
    if (!inTauri) return mockBackend().uninstallPackage(name);
    return window.__TAURI__.core.invoke("uninstall_package", { name });
  },

  /** @returns {Promise<NotebookInfo>} */
  startNotebook(workdir, openBrowser = false) {
    if (!inTauri) return mockBackend().startNotebook(workdir, openBrowser);
    return window.__TAURI__.core.invoke("start_notebook", { workdir, openBrowser });
  },

  stopNotebook() {
    if (!inTauri) return mockBackend().stopNotebook();
    return window.__TAURI__.core.invoke("stop_notebook");
  },

  defaultWorkdir() {
    if (!inTauri) return mockBackend().defaultWorkdir();
    return window.__TAURI__.core.invoke("default_workdir");
  },

  ensureWorkdir(saved) {
    if (!inTauri) return Promise.resolve(null);
    return window.__TAURI__.core.invoke("ensure_workdir", { saved: saved || null });
  },

  /** @returns {Promise<RecentItem[]>} */
  listRecent(workdir) {
    if (!inTauri) return mockBackend().listRecent(workdir);
    return window.__TAURI__.core.invoke("list_recent_notebooks", { workdir });
  },

  openNotebookUrl() {
    if (!inTauri) return mockBackend().openNotebookUrl();
    return window.__TAURI__.core.invoke("open_notebook_url");
  },

  openRecentNotebook(id) {
    if (!inTauri) return mockBackend().openRecentNotebook(id);
    return window.__TAURI__.core.invoke("open_recent_notebook", { id });
  },

  async resetEnv(onStage) {
    if (!inTauri) return mockBackend().resetEnv(onStage);
    const { listen } = window.__TAURI__.event;
    const un = await listen("reset-progress", e => onStage(e.payload.percent, e.payload.step));
    try { return await window.__TAURI__.core.invoke("reset_env"); }
    finally { un(); }
  },

  async pickDirectory() {
    if (!inTauri) return null;
    return window.__TAURI__.core.invoke("pick_directory");
  },

  /** @returns {Promise<string>} 应用版本号（构建产物元数据） */
  appVersion() {
    if (!inTauri) return Promise.resolve("dev");
    return window.__TAURI__.core.invoke("app_version");
  },

  /** 检查 GitHub 最新正式版 + 热修复清单（网络失败时 reject） */
  checkUpdates() {
    if (!inTauri) return mockBackend().checkUpdates();
    return window.__TAURI__.core.invoke("check_updates");
  },

  /** 应用指定热修复补丁 */
  applyPatch(id) {
    if (!inTauri) return mockBackend().applyPatch(id);
    return window.__TAURI__.core.invoke("apply_patch", { id });
  },
};

/* ================= 诊断上报（仅调试模式输出，生产构建完全静默） ================= */

export function reportDiag(stage, extra) {
  if (!debugMode) return;
  try {
    if (inTauri) {
      const cssLink = document.querySelector('link[href="styles.css"]');
      window.__TAURI__.core.invoke("diag_report", {
        msg: JSON.stringify(Object.assign({
          stage,
          cssLoaded: !!(cssLink && cssLink.sheet),
          screens: document.querySelectorAll(".screen").length,
          hiddenScreens: [...document.querySelectorAll(".screen")].filter(s => s.classList.contains("hidden")).length,
          overlays: document.querySelectorAll(".overlay").length,
          appJsExecuted: true,
        }, extra || {})),
      });
    } else {
      console.log("[diag]", stage, extra || "");
    }
  } catch (e) {}
}
