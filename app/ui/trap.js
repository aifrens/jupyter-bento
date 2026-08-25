/* 致命错误兜底（独立文件，先于 app.js 加载，可捕获 app.js 的解析/运行错误）
 * 任何未捕获异常都以红色横幅显示在界面顶部，避免白屏无提示。
 */
function jupiterFatal(msg) {
  try {
    var b = document.getElementById("fatalBanner");
    if (b) {
      b.style.display = "block";
      // Overlay 标题栏会让 WebView 延伸到窗口顶边，需要避开 macOS 红黄绿按钮。
      b.classList.toggle("fatal-banner-mac", /Mac OS|macOS|Macintosh/.test(navigator.userAgent));
      b.innerHTML = "";
      var main = document.createElement("div");
      main.className = "fatal-banner-title";
      main.textContent = "界面出现异常，请重启应用。若反复出现，请前往 GitHub 提交 Issue（https://github.com/aifrens/jupyter-bento/issues/new），并附上以下信息：";
      var detail = document.createElement("div");
      detail.className = "fatal-banner-detail";
      detail.textContent = String(msg);
      b.appendChild(main);
      b.appendChild(detail);
    }
    if (window.__TAURI__ && window.__TAURI__.core) {
      window.__TAURI__.core.invoke("diag_report", { msg: "FATAL " + msg });
    }
    document.title = "ERR " + String(msg).slice(0, 80);
  } catch (e) {}
}

window.addEventListener("error", function (e) {
  jupiterFatal(
    "界面脚本错误：" + (e.message || e.type) +
    " @" + (e.filename || "").split("/").pop() + ":" + (e.lineno || 0)
  );
});

window.addEventListener("unhandledrejection", function (e) {
  jupiterFatal("异步错误：" + (e.reason && e.reason.message ? e.reason.message : String(e.reason)));
});
