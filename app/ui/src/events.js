/* 事件系统（委托模式）
 * 所有交互通过 data-action 属性 + document 级委托派发。
 * 注意：不使用内联 onclick —— WebView 的 CSP 会拦截内联事件处理器，
 * 导致「页面能渲染但按钮全部无响应」（线上事故根因）。
 */

let actions = {};

/** 由 main.js 组装各页面处理器后注入 */
export function initActions(map) {
  actions = map;
}

document.addEventListener("click", e => {
  const el = e.target.closest("[data-action]");
  if (!el) return;
  const fn = actions[el.dataset.action];
  if (fn) fn(el);
});
