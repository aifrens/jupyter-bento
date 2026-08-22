/** 统一 toast 通知（替代原生 alert） */

export function showToast(msg, kind) {
  let host = document.getElementById("toastHost");
  if (!host) {
    host = document.createElement("div");
    host.id = "toastHost";
    host.style.cssText = "position:fixed;top:16px;left:50%;transform:translateX(-50%);z-index:10000;display:flex;flex-direction:column;gap:8px;align-items:center;pointer-events:none;";
    document.body.appendChild(host);
  }
  const t = document.createElement("div");
  t.className = "toast " + (kind === "error" ? "toast-error" : "toast-info");
  t.textContent = msg;
  host.appendChild(t);
  setTimeout(() => {
    t.classList.add("toast-out");
    setTimeout(() => t.remove(), 320);
  }, 4500);
}
