/** 页面路由：屏幕切换、弹窗层、初始化导航锁 */

const overlayIds = ["o-install", "o-installing", "o-reset", "o-resetting", "o-update"];

let navLocked = false;

/* 逐页滚动位置记忆：离开页面时记录 scrollTop，返回时恢复；
   未访问过的页面默认为 0（首次进入始终在顶部） */
const scrollMemory = { home: 0, env: 0, settings: 0 };
let currentScreen = null;

/* 页面进入钩子：各页面模块在此注册，避免 router ↔ pages 循环依赖 */
const enterHooks = {};

/** 注册页面进入回调（如 env → refreshPackages） */
export function onEnter(screen, fn) {
  (enterHooks[screen] ||= []).push(fn);
}

/** 初始化期间锁定/解锁侧边导航（防止用户在环境就绪前进入其他页面） */
export function lockNav(locked) {
  navLocked = locked;
  const nav = document.getElementById("mainNav");
  if (nav) nav.classList.toggle("nav-locked", locked);
}

export function go(screen) {
  if (navLocked && screen !== "first") return; // 初始化期间禁止离开（真实拦截，不止视觉置灰）
  // 离开前记录当前页的滚动位置
  if (currentScreen && currentScreen in scrollMemory) {
    const cur = document.getElementById("s-" + currentScreen);
    if (cur) scrollMemory[currentScreen] = cur.scrollTop;
  }
  document.querySelectorAll(".screen").forEach(s => { s.classList.add("hidden"); s.classList.remove("flex"); });
  closeOverlays();
  const el = document.getElementById("s-" + screen);
  el.classList.remove("hidden");
  if (screen === "first") el.classList.add("flex");
  // 恢复目标页的滚动位置（未访问过则为 0）
  if (screen in scrollMemory) {
    el.scrollTop = scrollMemory[screen] || 0;
  }
  currentScreen = screen;
  document.querySelectorAll(".nav-item").forEach(n => n.classList.toggle("active", n.dataset.screen === screen));
  (enterHooks[screen] || []).forEach(fn => fn());
}

export function showOverlay(id) {
  document.getElementById(id).classList.remove("hidden");
}

export function closeOverlays() {
  overlayIds.forEach(i => document.getElementById(i).classList.add("hidden"));
}
