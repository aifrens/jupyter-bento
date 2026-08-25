/** 首页：Hero 三态（待启动/启动中/运行中）、工作目录、最近打开、外链 */
import { backend, reportDiag, inTauri } from "../backend.js";
import { state, saveSettings } from "../state.js";
import { showToast } from "../toast.js";
import { fmtAgo, tpl } from "../util.js";
import { closeOverlays, onEnter, showOverlay } from "../router.js";

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

/* ================= 在线更新 ================= */

const SPINNER = '<svg class="w-4 h-4 animate-spin shrink-0" viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="10" stroke="currentColor" stroke-opacity="0.25" stroke-width="4"/><path d="M22 12a10 10 0 00-10-10" stroke="currentColor" stroke-width="4" stroke-linecap="round"/></svg>';

/** 后台静默检查更新（网络失败不打扰用户，仅在侧栏留轻量重试入口） */
export async function checkUpdatesInBackground() {
  if (!inTauri) return;
  const retryEl = document.getElementById("updateRetry");
  try {
    const info = await backend.checkUpdates();
    const has = !!info.latest_version || (info.patches && info.patches.length > 0);
    state.update = has ? info : null;
    reportDiag("update-check", {
      latest: info.latest_version || null,
      patchCount: info.patches ? info.patches.length : 0,
    });
    if (retryEl) retryEl.classList.add("hidden");
    renderUpdateBadge();
  } catch (e) {
    reportDiag("update-check-failed", { error: String(e) });
    if (retryEl) retryEl.classList.remove("hidden");
  }
}

function renderUpdateBadge() {
  const badge = document.getElementById("updateBadge");
  if (!badge) return;
  if (state.update) {
    document.getElementById("updateBadgeText").textContent =
      state.update.latest_version ? "有新版本" : "有修复";
    badge.classList.remove("hidden");
    badge.classList.add("inline-flex");
  } else {
    badge.classList.add("hidden");
    badge.classList.remove("inline-flex");
  }
}

export function openUpdate() {
  renderUpdateDialog();
  showOverlay("o-update");
}

function setActions(buttons) {
  document.getElementById("updateActions").innerHTML = buttons
    .map(b => `<button class="${b.primary ? "btn-primary" : "btn-ghost"}" data-action="${b.action}">${b.label}</button>`)
    .join("");
}

function renderUpdateDialog() {
  const title = document.getElementById("updateTitle");
  const body = document.getElementById("updateBody");
  const info = state.update;
  if (!info) {
    title.textContent = "检查更新";
    body.innerHTML = `<div class="flex items-center gap-2.5 text-[13px] text-slate-500">${SPINNER}正在获取更新信息…</div>`;
    setActions([{ label: "关闭", action: "closeOverlays", primary: false }]);
    return;
  }
  if (info.latest_version) {
    title.textContent = `发现新版本 v${info.latest_version}`;
    body.innerHTML = `
      <p class="text-[13px] text-slate-500 mb-3">当前版本 v${state.appVersion || "—"}。更新需要下载新版安装包（无法在线热更）：</p>
      <div class="text-xs font-medium text-slate-400 mb-1.5">更新日志</div>
      <div id="updateNotes" class="max-h-56 overflow-y-auto bg-slate-50 border border-slate-100 rounded-xl p-4 text-[13px] text-slate-600 whitespace-pre-wrap leading-relaxed"></div>`;
    body.querySelector("#updateNotes").textContent = info.release_notes || "（无更新日志）";
    setActions([
      { label: "稍后", action: "closeOverlays", primary: false },
      { label: "前往 GitHub 下载", action: "openUpdateDownload", primary: true },
    ]);
  } else {
    title.textContent = "有可用运行时修复";
    body.innerHTML = `
      <p class="text-[13px] text-slate-500 mb-3">以下修复可在线应用（体积小，无需下载安装包）：</p>
      <div class="space-y-2 max-h-56 overflow-y-auto">
        ${info.patches.map(p => `
          <div class="bg-slate-50 border border-slate-100 rounded-xl p-3.5">
            <div class="text-[13px] font-medium text-slate-700">${p.title}</div>
            <div class="text-xs text-slate-400 mt-1 leading-relaxed">${p.description}</div>
          </div>`).join("")}
      </div>`;
    setActions([
      { label: "稍后", action: "closeOverlays", primary: false },
      { label: "立即修复", action: "applyUpdates", primary: true },
    ]);
  }
}

export function openUpdateDownload() {
  if (state.update && state.update.release_url) {
    openLink(state.update.release_url);
  }
  closeOverlays();
}

export async function applyUpdates() {
  const info = state.update;
  if (!info || !info.patches || !info.patches.length) return;
  const body = document.getElementById("updateBody");
  body.innerHTML = `<div class="flex items-center gap-2.5 text-[13px] text-slate-500">${SPINNER}正在下载并应用修复（国内网络可能较慢，请稍候）…</div>`;
  setActions([]);
  try {
    for (const p of info.patches) {
      await backend.applyPatch(p.id);
    }
    showToast("修复完成，已自动应用运行时更新", "info");
    state.update = null;
    renderUpdateBadge();
    closeOverlays();
  } catch (e) {
    showToast("修复失败：" + e + "（可稍后重试）", "error");
    renderUpdateDialog();
  }
}

/** 手动重试（网络失败后的用户入口，含加载/错误/成功三态） */
export async function retryUpdate() {
  const title = document.getElementById("updateTitle");
  const body = document.getElementById("updateBody");
  title.textContent = "正在检查更新…";
  body.innerHTML = `<div class="flex items-center gap-2.5 text-[13px] text-slate-500">${SPINNER}正在连接 GitHub…</div>`;
  setActions([]);
  showOverlay("o-update");
  try {
    const info = await backend.checkUpdates();
    const has = !!info.latest_version || (info.patches && info.patches.length > 0);
    state.update = has ? info : null;
    if (has) {
      renderUpdateBadge();
      renderUpdateDialog();
    } else {
      title.textContent = "已是最新";
      body.innerHTML = `<p class="text-[13px] text-slate-500">当前已是最新版本，也没有待修复的运行时补丁。</p>`;
      setActions([{ label: "好的", action: "closeOverlays", primary: true }]);
    }
    const retryEl = document.getElementById("updateRetry");
    if (retryEl) retryEl.classList.add("hidden");
  } catch (e) {
    title.textContent = "无法连接 GitHub";
    body.innerHTML = `
      <div class="flex items-start gap-3">
        <div class="w-9 h-9 shrink-0 rounded-full bg-amber-50 flex items-center justify-center text-amber-500">!</div>
        <div class="text-[13px] text-slate-500 leading-relaxed">
          检查更新失败（国内网络访问 GitHub 可能受限）。<br>
          请检查网络/代理后重试，或稍后再试。<br>
          <span class="text-xs text-slate-400 mt-1 block">${String(e)}</span>
        </div>
      </div>`;
    setActions([
      { label: "取消", action: "closeOverlays", primary: false },
      { label: "重试", action: "retryUpdate", primary: true },
    ]);
  }
}

// 路由进入首页时刷新内容
onEnter("home", renderHome);
