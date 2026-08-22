/** 环境管理页：包列表 / 安装新包 / 镜像源卡片 */
import { backend, reportDiag } from "../backend.js";
import { state } from "../state.js";
import { tpl } from "../util.js";
import { go, showOverlay, closeOverlays, onEnter } from "../router.js";

export const MIRRORS = [
  { id: "aliyun", name: "阿里云镜像", tag: "默认 · 推荐", url: "https://mirrors.aliyun.com/pypi/simple/" },
  { id: "tsinghua", name: "清华大学镜像", tag: "", url: "https://pypi.tuna.tsinghua.edu.cn/simple" },
  { id: "pypi", name: "PyPI 官方源", tag: "", url: "https://pypi.org/simple" },
];

let selectedMirror = state.settings.mirror;

export async function refreshPackages() {
  try { state.packages = await backend.listPackages(); } catch { state.packages = []; }
  renderPkgTable();
}

export function renderPkgTable() {
  const q = (document.getElementById("pkgSearch").value || "").toLowerCase();
  const rows = state.packages
    .filter(p => !q || p.name.toLowerCase().includes(q))
    .sort((a, b) => (a.builtin === b.builtin ? a.name.localeCompare(b.name) : a.builtin ? 1 : -1));

  const fragment = document.createDocumentFragment();
  for (const p of rows) {
    const row = tpl("tpl-pkg-row");
    row.querySelector('[data-field="name"]').textContent = p.name;
    row.querySelector('[data-field="version"]').textContent = p.version;
    if (p.builtin) {
      row.querySelector('[data-field="badge-user"]').remove();
      row.querySelector('[data-field="action-user"]').remove();
    } else {
      row.querySelector('[data-field="badge-builtin"]').remove();
      row.querySelector('[data-field="action-builtin"]').remove();
      row.querySelector('[data-field="action-user"]').dataset.arg = p.name;
    }
    fragment.appendChild(row);
  }
  document.getElementById("pkgTableBody").replaceChildren(fragment);

  const user = state.packages.filter(p => !p.builtin).length;
  const builtin = state.packages.length - user;
  document.getElementById("statUser").textContent = user;
  document.getElementById("statBuiltin").textContent = builtin;
  const bc1 = document.getElementById("resetBuiltinCount");
  if (bc1) bc1.textContent = builtin;
  const bc2 = document.getElementById("dangerBuiltinCount");
  if (bc2) bc2.textContent = builtin;
  reportDiag("pkg-counts", { total: state.packages.length, builtin, user });
  document.getElementById("pkgSummary").textContent =
    `共 ${state.packages.length} 个包（内置 ${builtin} · 用户安装 ${user}） · 内置包为出厂环境（含锁定包及其依赖），不可卸载，保证环境稳定。`;
}

export async function uninstallPkg(name) {
  await backend.uninstallPackage(name);
  await refreshPackages();
}

/** 构建镜像卡片列表（设置页与安装弹窗共用） */
export function buildMirrorCards(selectedUrl, action) {
  const fragment = document.createDocumentFragment();
  for (const m of MIRRORS) {
    const node = tpl("tpl-mirror-card");
    const el = node.querySelector(".mirror-option");
    el.dataset.action = action;
    el.dataset.arg = m.url;
    node.querySelector('[data-field="name"]').textContent = m.name;
    node.querySelector('[data-field="url"]').textContent = m.url;
    const tagEl = node.querySelector('[data-field="tag"]');
    if (m.tag) {
      tagEl.textContent = m.tag;
    } else {
      tagEl.remove();
    }
    if (selectedUrl === m.url) {
      el.classList.add("selected");
      el.querySelector('[data-field="dot"]').classList.replace("border-slate-300", "border-brand");
    } else {
      el.querySelector('[data-field="dot-inner"]').remove();
    }
    fragment.appendChild(node);
  }
  return fragment;
}

export function openInstall() {
  selectedMirror = state.settings.mirror;
  renderMirrorOptions();
  showOverlay("o-install");
  setTimeout(() => document.getElementById("installName").focus(), 60);
}

function renderMirrorOptions() {
  document.getElementById("mirrorOptions").replaceChildren(
    buildMirrorCards(selectedMirror, "selectMirror")
  );
}

export function selectMirror(url) {
  selectedMirror = url;
  renderMirrorOptions();
}

function termLog(text, kind) {
  const body = document.getElementById("termBody");
  const div = document.createElement("div");
  div.className = "terminal-line " + (kind === "ok" ? "text-emerald-400" : kind === "err" ? "text-red-400" : kind === "dim" ? "text-slate-500" : "text-slate-300");
  div.textContent = text;
  const caret = body.querySelector(".caret");
  if (caret) caret.remove();
  body.appendChild(div);
  const c = document.createElement("span");
  c.className = "caret text-slate-500 animate-blink";
  c.textContent = "▍";
  body.appendChild(c);
  body.scrollTop = body.scrollHeight;
}

export async function doInstall() {
  const name = document.getElementById("installName").value.trim();
  const version = document.getElementById("installVersion").value.trim();
  if (!name) {
    document.getElementById("installName").classList.add("!border-red-300", "!ring-4", "!ring-red-500/10");
    document.getElementById("installName").focus();
    return;
  }
  const spec = version ? `${name}==${version}` : name;
  closeOverlays();
  showOverlay("o-installing");
  document.getElementById("installingTitle").textContent = `正在安装 ${name}…`;
  document.getElementById("termBody").innerHTML = "";
  document.getElementById("installResult").classList.add("hidden");
  document.getElementById("installFinishBtn").disabled = true;

  try {
    await backend.installPackage(spec, termLog);
    showInstallResult(true, `安装成功！${spec} 已加入环境。`);
  } catch (e) {
    termLog("ERROR: " + e, "err");
    showInstallResult(false, "安装失败。请检查包名/版本是否正确，或更换安装源后重试。详细原因见上方日志。");
  }
}

function showInstallResult(ok, msg) {
  const box = document.getElementById("installResult");
  box.className = ok
    ? "mt-3.5 flex items-center gap-2.5 text-[13px] text-emerald-700 bg-emerald-50 border border-emerald-100 rounded-xl px-4 py-3 animate-fade-up"
    : "mt-3.5 flex items-start gap-2.5 text-[13px] text-red-600 bg-red-50 border border-red-100 rounded-xl px-4 py-3 animate-fade-up";
  box.innerHTML = (ok
    ? '<svg viewBox="0 0 24 24" class="w-4 h-4 shrink-0" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 12.75l6 6 9-13.5"/></svg>'
    : '<svg viewBox="0 0 24 24" class="w-4 h-4 shrink-0 mt-px" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 8v5m0 3h.01"/></svg>') + `<span>${msg}</span>`;
  document.getElementById("installingTitle").textContent = ok ? "安装完成" : "安装失败";
  document.getElementById("installFinishBtn").disabled = false;
}

export async function finishInstall() {
  closeOverlays();
  document.getElementById("installName").value = "";
  document.getElementById("installVersion").value = "";
  go("env");
}

// 路由进入环境页时刷新包列表
onEnter("env", refreshPackages);
