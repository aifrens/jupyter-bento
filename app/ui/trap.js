/* 致命错误兜底（独立文件，先于 app.js 加载，可捕获 app.js 的解析/运行错误）
 * 任何未捕获异常都以红色横幅显示在界面顶部，避免白屏无提示。
 */
function jupiterFatal(msg) {
  try {
    var b = document.getElementById("fatalBanner");
    if (b) {
      b.style.display = "block";
      b.innerHTML = "";
      var main = document.createElement("div");
      main.style.fontWeight = "600";
      main.textContent = "界面出现异常，请重启应用。若反复出现，请联系支持并提供以下信息：";
      var detail = document.createElement("div");
      detail.style.cssText = "opacity:.8;font-size:12px;margin-top:2px;font-family:ui-monospace,monospace;word-break:break-all;";
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
