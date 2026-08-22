/** 首页：Hero 三态（待启动/启动中/运行中）、工作目录、最近打开、外链 */
import { backend, reportDiag, inTauri } from "../backend.js";
import { state, saveSettings } from "../state.js";
import { showToast } from "../toast.js";
import { fmtAgo, tpl } from "../util.js";
import { onEnter } from "../router.js";

// 首页内容区使用"行星砖"方案 2 图标；左侧导航图标保持 index.html 原样。
const HOME_ICON_SRC = {
  recent: "icons/option-2/recent.svg",
};

/** 复用进行中的启动请求，避免多个入口并发创建 Notebook 进程 */
let notebookStartPromise = null;

/** 最近记录只在当前渲染批次内按后端 opaque id 索引，不持久化文件路径 */
let recentById = new Map();
let recentRequestSeq = 0;
const openingRecentIds = new Set();

function createHomeIcon(className) {
  const img = document.createElement("img");
  img.src = HOME_ICON_SRC.recent;
  img.alt = "";
  img.draggable = false;
  img.setAttribute("aria-hidden", "true");
  img.className = className;
  return img;
}

export function renderHome() {
  const hero = document.getElementById("heroCard");
  const running = !!state.running;
  if (running) {
    hero.className = "card mt-7 p-6 transition-colors duration-300 !border-emerald-200/80";
    const node = tpl("tpl-hero-running");
    node.querySelector('[data-field="url"]').textContent = state.running.url;
    hero.replaceChildren(node);
  } else if (state.starting) {
    hero.className = "card mt-7 p-6 flex items-center gap-6 transition-colors duration-300 !border-amber-200/70";
    hero.replaceChildren(tpl("tpl-hero-starting"));
  } else {
    hero.className = "hero-idle mt-7 p-7 rounded-2xl flex items-center gap-6";
    hero.replaceChildren(tpl("tpl-hero-idle"));
  }
  document.getElementById("workdirLabel").textContent = state.settings.workdir;
  // 服务运行中禁止更改工作目录（运行中的服务仍指向旧目录，会造成状态不一致）
  const wdBtn = document.getElementById("workdirBtn");
  const wdHint = document.getElementById("workdirHint");
  if (wdBtn) wdBtn.disabled = running || state.starting;
  if (wdHint) {
    wdHint.textContent = running || state.starting
      ? "服务启动或运行期间，如需更改目录，请先停止服务。"
      : "您的 .ipynb 笔记文件都保存在此目录中，重置环境不会影响这些文件。";
  }
  renderRecent();
}

/** 确保 Notebook 已启动；同一时刻所有调用方共享一份启动结果。 */
async function ensureNotebookStarted(workdir) {
  if (state.running) return state.running;
  if (notebookStartPromise) return notebookStartPromise;
  if (inTauri && !state.envReady) {
    throw new Error("环境尚未就绪，请等待初始化完成");
  }
  state.starting = true;
  renderHome();
  notebookStartPromise = backend.startNotebook(workdir, false)
    .then(info => {
      state.running = info;
      return info;
    })
    .finally(() => {
      state.starting = false;
      notebookStartPromise = null;
      renderHome();
    });
  return notebookStartPromise;
}

export async function startNotebook() {
  if (state.running || state.starting) return;
  try {
    await ensureNotebookStarted(state.settings.workdir);
    await backend.openNotebookUrl();
  } catch (e) {
    showToast("启动失败：" + e, "error");
  }
}

export async function stopNotebook() {
  await backend.stopNotebook();
  state.running = null;
  notebookStartPromise = null;
  renderHome();
}

export async function openInBrowser() {
  if (!state.running) return;
  try {
    await backend.openNotebookUrl();
  } catch (e) {
    showToast("打开失败：" + e, "error");
  }
}

export function copyUrl() {
  if (state.running) navigator.clipboard && navigator.clipboard.writeText(state.running.url);
}

export async function pickWorkdir() {
  if (state.running || state.starting) return; // 启动或运行中禁止更改（按钮已置灰，双保险）
  const dir = await backend.pickDirectory();
  if (dir) {
    recentRequestSeq += 1;
    recentById = new Map();
    state.settings.workdir = dir;
    saveSettings(state.settings);
    renderHome();
  }
}

export async function renderRecent() {
  const list = document.getElementById("recentList");
  if (!list) return;
  const workdir = state.settings.workdir;
  const requestSeq = ++recentRequestSeq;
  let items = [];
  try {
    items = await backend.listRecent(workdir);
  } catch (e) {
    if (requestSeq !== recentRequestSeq || state.settings.workdir !== workdir) return;
    reportDiag("recent-list-failed", { workdir, error: String(e) });
  }
  if (requestSeq !== recentRequestSeq || state.settings.workdir !== workdir) return;
  items = items.filter(item => item && item.id);
  recentById = new Map(items.map(item => [item.id, item]));
  list.replaceChildren();
  if (!items.length) {
    list.className = "mt-3 rounded-2xl border-2 border-dashed border-slate-200 bg-slate-50/40 px-6 py-8 text-center";
    const icon = document.createElement("div");
    icon.className = "w-10 h-10 mx-auto flex items-center justify-center mb-2.5";
    icon.setAttribute("aria-hidden", "true");
    icon.appendChild(createHomeIcon("w-7 h-7"));
    const title = document.createElement("div");
    title.className = "text-sm text-slate-400";
    title.textContent = "暂无最近文件";
    const hint = document.createElement("div");
    hint.className = "text-xs text-slate-400/80 mt-1";
    hint.textContent = "在浏览器中打开的 .ipynb 会出现在这里";
    list.append(icon, title, hint);
    return;
  }
  list.className = "card mt-3 divide-y divide-slate-50 overflow-hidden";
  const fragment = document.createDocumentFragment();
  for (const item of items) {
    const row = document.createElement("div");
    row.className = "flex items-center gap-3 px-6 py-3.5 hover:bg-brand-soft/50 cursor-pointer transition-colors group";
    row.dataset.action = "openRecent";
    row.dataset.recentId = item.id;
    row.title = item.name;

    const icon = document.createElement("span");
    icon.className = "w-8 h-8 flex items-center justify-center shrink-0";
    icon.setAttribute("aria-hidden", "true");
    icon.appendChild(createHomeIcon("w-5 h-5"));
    const name = document.createElement("span");
    name.className = "text-sm text-slate-700 flex-1 truncate group-hover:text-slate-900 transition-colors";
    name.textContent = item.name;
    const time = document.createElement("span");
    time.className = "text-xs text-slate-400 shrink-0";
    time.textContent = fmtAgo(item.modified_ms);
    row.append(icon, name, time);
    fragment.appendChild(row);
  }
  list.appendChild(fragment);
}

function isStaleRecentError(error) {
  return /^RECENT_(?:NOT_FOUND|WORKDIR_MISMATCH|INVALID_PATH|FILE_NOT_FOUND):/.test(String(error));
}

export async function openRecent(id) {
  const item = recentById.get(id);
  if (!item || openingRecentIds.has(id)) return;
  const requestedWorkdir = state.settings.workdir;
  openingRecentIds.add(id);
  try {
    await ensureNotebookStarted(requestedWorkdir);
    if (!recentById.has(id) || state.settings.workdir !== requestedWorkdir) return;
    await backend.openRecentNotebook(id);
  } catch (e) {
    showToast("打开失败：" + e, "error");
    if (isStaleRecentError(e)) await renderRecent();
  } finally {
    openingRecentIds.delete(id);
  }
}

/** 在系统浏览器中打开外部链接（版本号 → 项目仓库） */
export function openLink(url) {
  if (inTauri) {
    window.__TAURI__.core.invoke("open_external_url", { url }).catch(e => showToast("打开链接失败：" + e, "error"));
  } else {
    window.open(url, "_blank");
  }
}

// 路由进入首页时刷新内容
onEnter("home", renderHome);
