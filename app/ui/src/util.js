/** 通用小工具（无依赖） */

export function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

/** 克隆 <template> 内容（HTML 模板集中管理的配套工具） */
export function tpl(id) {
  return document.getElementById(id).content.cloneNode(true);
}

/** 时间戳 → 中文相对时间 */
export function fmtAgo(ms) {
  const s = Math.floor((Date.now() - ms) / 1000);
  if (s < 60) return "刚刚";
  if (s < 3600) return Math.floor(s / 60) + " 分钟前";
  if (s < 86400) return Math.floor(s / 3600) + " 小时前";
  if (s < 172800) return "昨天";
  if (s < 2592000) return Math.floor(s / 86400) + " 天前";
  return new Date(ms).toLocaleDateString("zh-CN");
}
