/** 全局应用状态与设置持久化 */

/**
 * @typedef {Object} AppSettings
 * @property {string} mirror  默认 PyPI 安装源
 * @property {string} workdir Notebook 工作目录
 */

const DEFAULTS = {
  mirror: "https://mirrors.aliyun.com/pypi/simple/",
  workdir: "~/Jupiter/notebooks",
};

/** @returns {AppSettings} */
export function loadSettings() {
  try {
    return Object.assign({}, DEFAULTS, JSON.parse(localStorage.getItem("jupiter-settings") || "{}"));
  } catch {
    return { ...DEFAULTS };
  }
}

/** @param {AppSettings} settings */
export function saveSettings(settings) {
  localStorage.setItem("jupiter-settings", JSON.stringify(settings));
}

export const state = {
  /** @type {import("./backend.js").Pkg[]} */
  packages: [],
  /** @type {import("./backend.js").NotebookInfo | null} */
  running: null,
  starting: false,
  envReady: false,
  /** @type {AppSettings} */
  settings: loadSettings(),
};
