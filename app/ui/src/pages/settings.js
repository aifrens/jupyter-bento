/** 设置页：默认安装源（卡片）/ 一键重置 */
import { backend } from "../backend.js";
import { state, saveSettings } from "../state.js";
import { go, showOverlay, closeOverlays } from "../router.js";
import { buildMirrorCards } from "./env.js";

// 设置页：默认安装源（卡片式选择，与安装弹窗统一）
export function renderSettingsMirror() {
  const box = document.getElementById("settingsMirrorOptions");
  if (!box) return;
  box.replaceChildren(buildMirrorCards(state.settings.mirror, "selectSettingsMirror"));
}

export function selectSettingsMirror(url) {
  state.settings.mirror = url;
  saveSettings(state.settings);
  renderSettingsMirror();
}

export function openReset() {
  const user = state.packages.filter(p => !p.builtin);
  document.getElementById("resetUserCount").textContent = user.length;
  document.getElementById("resetUserList").textContent = user.length ? "：" + user.map(p => p.name).join("、") : "";
  showOverlay("o-reset");
}

export async function doReset() {
  closeOverlays();
  showOverlay("o-resetting");
  document.getElementById("resetTitle").textContent = "正在重置环境…";
  document.getElementById("resetOk").classList.add("hidden");
  document.getElementById("resetFinishBtn").disabled = true;
  const bar = document.getElementById("resetBar");
  bar.style.width = "0%";
  const t0 = Date.now();
  try {
    await backend.resetEnv((p, step) => {
      bar.style.width = p + "%";
      document.getElementById("resetStep").textContent = step;
    });
    state.running = null;
    state.envReady = true; // 重置完成即恢复到厂环境，可直接启动
    document.getElementById("resetTitle").textContent = "重置完成";
    document.getElementById("resetStep").textContent = `共耗时 ${((Date.now() - t0) / 1000).toFixed(1)} 秒`;
    document.getElementById("resetOk").classList.remove("hidden");
  } catch (e) {
    document.getElementById("resetTitle").textContent = "重置失败";
    document.getElementById("resetStep").textContent = String(e);
  }
  document.getElementById("resetFinishBtn").disabled = false;
}

export function finishReset() {
  closeOverlays();
  state.packages = [];
  go("home");
}
