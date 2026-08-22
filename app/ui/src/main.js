/* 朱比特和它的朋友们 · 入口装配
 * 组装事件处理器表 → 初始化事件委托 → 启动引导
 */
import { backend, reportDiag, mockReady, mockBackend, setDebugMode, debugMode, inTauri } from "./backend.js";
import { state, saveSettings } from "./state.js";
import { go, lockNav, closeOverlays } from "./router.js";
import { initActions } from "./events.js";
import {
  renderHome, renderRecent, startNotebook, stopNotebook, openInBrowser,
  copyUrl, pickWorkdir, openRecent, openLink,
} from "./pages/home.js";
import {
  refreshPackages, renderPkgTable, uninstallPkg, openInstall, selectMirror,
  doInstall, finishInstall, MIRRORS,
} from "./pages/env.js";
import { renderSettingsMirror, selectSettingsMirror, openReset, doReset, finishReset } from "./pages/settings.js";

/* ================= 事件处理器注册（与 data-action 对应） ================= */

initActions({
  go: el => go(el.dataset.arg),
  startNotebook: () => startNotebook(),
  stopNotebook: () => stopNotebook(),
  openInBrowser: () => openInBrowser(),
  copyUrl: () => copyUrl(),
  pickWorkdir: () => pickWorkdir(),
  openInstall: () => openInstall(),
  selectMirror: el => selectMirror(el.dataset.arg),
  selectSettingsMirror: el => selectSettingsMirror(el.dataset.arg),
  closeOverlays: () => closeOverlays(),
  doInstall: () => doInstall(),
  finishInstall: () => finishInstall(),
  openReset: () => openReset(),
  doReset: () => doReset(),
  finishReset: () => finishReset(),
  uninstallPkg: el => uninstallPkg(el.dataset.arg),
  openRecent: el => openRecent(el.dataset.recentId),
  openLink: el => openLink(el.dataset.arg),
});

/* ================= 启动流程 ================= */

async function boot() {
  // 非 macOS 平台保留原生标题栏：隐藏自定义拖动区
  if (!/Mac OS|macOS|Macintosh/.test(navigator.userAgent)) {
    document.body.classList.add("non-mac");
  }
  // 预览模式：等待 mock 后端注入（正式产物无此文件，会明确报错而非静默假数据）
  if (!inTauri) {
    try {
      await mockReady;
    } catch (e) {
      jupiterFatal(String(e.message || e));
      return;
    }
  }
  // 调试模式探测 + 真实环境路径展示 + 动态版本号（取自构建产物元数据）
  if (inTauri) {
    try { setDebugMode(await window.__TAURI__.core.invoke("debug_mode")); } catch (e) {}
    window.__TAURI__.core.invoke("get_env_path").then(p => {
      const el = document.getElementById("envPathLabel");
      if (el && p) el.textContent = p;
    }).catch(() => {});
  }
  backend.appVersion().then(v => {
    for (const id of ["appVersionLabel", "appVersionLabel2"]) {
      const el = document.getElementById(id);
      if (el) el.textContent = "v" + v;
    }
  }).catch(() => {});
  reportDiag("boot-start", { inTauri });
  renderSettingsMirror();
  document.getElementById("pkgSearch").addEventListener("input", renderPkgTable);
  // 工作目录校验：不存在/不可创建/外接卷未挂载时回退默认目录。
  // 注意不写入 localStorage —— 外接盘恢复后下次启动自动用回保存的路径。
  if (inTauri) {
    try {
      const saved = state.settings.workdir;
      const wd = await backend.ensureWorkdir(saved);
      if (wd) {
        state.settings.workdir = wd;
        if (saved && wd !== saved) {
          reportDiag("workdir-fallback", { saved, effective: wd });
        } else {
          reportDiag("workdir", { effective: wd });
        }
      }
    } catch (e) {
      reportDiag("ensure-workdir-failed", { error: String(e) });
    }
  }
  renderHome();
  const envReady = inTauri ? false : mockBackend().envReady; // 预览模式跳过初始化
  if (!envReady) {
    go("first");
    lockNav(true); // 初始化期间禁止导航，避免进入未就绪状态页面
    reportDiag("first-shown");
    document.querySelectorAll(".nav-item").forEach(n => n.classList.remove("active"));
    try {
      await backend.ensureEnv((p, step) => {
        document.getElementById("firstBar").style.width = p + "%";
        document.getElementById("firstPct").textContent = p + "%";
        document.getElementById("firstStep").textContent = step;
      });
      state.envReady = true;
    } catch (e) {
      reportDiag("ensure-env-failed", { error: String(e) });
      document.getElementById("firstStep").textContent = "环境初始化失败：" + e;
      lockNav(false); // 失败时解锁：用户可到设置页执行"一键重置"自救
      return;
    }
  } else {
    state.envReady = true;
  }
  lockNav(false);
  go("home");
  reportDiag("home-shown");
  if (inTauri) {
    window.__TAURI__.event.listen("notebook-exit", () => {
      state.running = null;
      state.starting = false;
      renderHome();
    });
    window.__TAURI__.event.listen("recent-changed", () => {
      if (!document.getElementById("s-home").classList.contains("hidden")) renderRecent();
    });
  }
  // 界面自检仅在调试模式运行（普通用户启动零跳转）
  if (debugMode) selfTest();
}

/* ================= 自检（仅调试模式；结果经 diag 输出到 stderr） ================= */

async function selfTest() {
  // 1) 读取 WebView 实际下发的 CSP
  try {
    fetch(location.href, { method: "HEAD" })
      .then(r => reportDiag("csp-header", { csp: r.headers.get("content-security-policy") || "(无)" }))
      .catch(e => reportDiag("csp-header", { error: String(e) }));
  } catch (e) {}

  // 2b) 逐页滚动位置记忆验证：settings 滚动后切走应互不影响，返回应恢复
  setTimeout(() => {
    try {
      const nav = s => document.querySelector(`[data-screen="${s}"]`);
      const settingsEl = document.getElementById("s-settings");
      const envEl = document.getElementById("s-env");
      nav("settings").click();
      settingsEl.scrollTop = 120;
      const expected = settingsEl.scrollTop; // 浏览器钳制后的实际值（内容不足以滚到 120 时会取最大可滚值）
      nav("env").click();
      const envScrollAfterSwitch = envEl.scrollTop;
      nav("settings").click();
      const settingsRestored = settingsEl.scrollTop;
      nav("home").click();
      reportDiag("selftest-scroll", {
        envScrollAfterSwitch, settingsRestored, expected,
        ok: envScrollAfterSwitch === 0 && settingsRestored === expected && expected > 0,
      });
    } catch (e) {
      reportDiag("selftest-scroll", { error: String(e) });
    }
  }, 1800);

  // 2) 程序化点击，验证事件委托真实生效（等同用户点击）
  setTimeout(() => {
    try {
      const nav = s => document.querySelector(`[data-screen="${s}"]`);
      const active = () => {
        const a = document.querySelector(".nav-item.active");
        return a ? a.dataset.screen : null;
      };
      const before = active();
      nav("env").click();
      setTimeout(() => {
        const mid = active();
        nav("settings").click();
        setTimeout(() => {
          const settingsScreen = active();
          const opts = document.querySelectorAll("#settingsMirrorOptions .mirror-option");
          const optCount = opts.length;
          let mirrorChanged = false;
          if (opts[1]) {
            opts[1].click();
            mirrorChanged = state.settings.mirror === MIRRORS[1].url;
          }
          // 还原默认镜像（不影响用户配置）
          state.settings.mirror = MIRRORS[0].url;
          saveSettings(state.settings);
          renderSettingsMirror();
          nav("home").click();
          setTimeout(() => {
            const after = active();
            reportDiag("selftest-click", {
              before, mid, settingsScreen, after,
              mirrorOpts: optCount, mirrorChanged,
              ok: before === "home" && mid === "env" && settingsScreen === "settings" && after === "home" && optCount === 3 && mirrorChanged,
            });
          }, 250);
        }, 250);
      }, 250);
    } catch (e) {
      reportDiag("selftest-error", { error: String(e) });
    }
  }, 600);

  // 3) Notebook 启停自检：仅调试模式启用（不开浏览器）
  if (inTauri) {
    try {
      reportDiag("nb-selftest-begin", { workdir: state.settings.workdir });
      const r = await backend.startNotebook(state.settings.workdir, false);
      state.running = r;
      renderHome();
      reportDiag("nb-start", { ok: true, url: r.url });
      reportDiag("workdir-btn-while-running", { disabled: !!document.getElementById("workdirBtn").disabled });
      // 页面 handler 成功回调后，recent-changed 会刷新 UI；此处仅延迟读取用于诊断持久化结果。
      setTimeout(async () => {
        try {
          const items = await backend.listRecent(state.settings.workdir);
          reportDiag("recent-count", { count: items.length, names: items.map(i => i.name).slice(0, 3) });
        } catch (e) {
          reportDiag("recent-count", { error: String(e) });
        }
      }, 7500);
      setTimeout(async () => {
        await backend.stopNotebook();
        state.running = null;
        renderHome();
        reportDiag("nb-stop", { ok: true, workdirBtnDisabledAfterStop: !!document.getElementById("workdirBtn").disabled });
      }, 9000);
    } catch (e) {
      reportDiag("nb-start", { ok: false, error: String(e) });
    }
  }
}

boot();
