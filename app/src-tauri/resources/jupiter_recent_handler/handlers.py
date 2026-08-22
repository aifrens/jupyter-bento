"""在 Notebook 页面确认可打开后，将相对路径回调给桌面应用。"""

from __future__ import absolute_import

import json
import os
import threading
try:
    from urllib.request import Request, urlopen
except ImportError:  # pragma: no cover - 仅保留 Python 2 导入兼容性
    from urllib2 import Request, urlopen

from notebook.base.handlers import FilesRedirectHandler, path_regex
from notebook.notebook.handlers import NotebookHandler, get_frontend_exporters
from notebook.utils import maybe_future
from tornado import gen, web


CALLBACK_URL_ENV = "JUPITER_RECENT_CALLBACK_URL"
CALLBACK_TOKEN_ENV = "JUPITER_RECENT_CALLBACK_TOKEN"
CALLBACK_TIMEOUT_SECONDS = 1.0


def _callback_config():
    """读取回环回调配置；缺少任一值时关闭上报。"""
    url = os.environ.get(CALLBACK_URL_ENV, "").strip()
    token = os.environ.get(CALLBACK_TOKEN_ENV, "").strip()
    if not url or not token:
        return None
    return url, token


def _post_recent_path(url, token, path):
    """向桌面应用上报路径；网络失败不会影响 Notebook 页面响应。"""
    payload = json.dumps({"path": path}, ensure_ascii=False).encode("utf-8")
    request = Request(url, data=payload)
    request.add_header("Authorization", "Bearer " + token)
    request.add_header("Content-Type", "application/json; charset=utf-8")
    request.add_header("Content-Length", str(len(payload)))
    response = urlopen(request, timeout=CALLBACK_TIMEOUT_SECONDS)
    try:
        response.read()
    finally:
        response.close()


def report_recent_path(path, log=None):
    """后台执行回调，确保应用端异常不会拖慢或阻断 Notebook 页面。"""
    config = _callback_config()
    if config is None:
        return

    def send():
        try:
            _post_recent_path(config[0], config[1], path)
        except Exception as exc:
            if log is not None:
                log.debug("recent-file callback failed for %s: %s", path, exc)

    worker = threading.Thread(target=send, name="jupiter-recent-callback")
    worker.daemon = True
    worker.start()


class RecentNotebookHandler(NotebookHandler):
    """优先接管 /notebooks，仅在确认模型为 notebook 后上报。"""

    @web.authenticated
    @gen.coroutine
    def get(self, path):
        # 与 Notebook 6.4.8 NotebookHandler.get 保持相同的读取、重定向和
        # 渲染语义；不能仅以父 Handler 正常返回判断，因为非 notebook 文件
        # 也会以成功重定向结束。
        path = path.strip("/")
        contents_manager = self.contents_manager
        try:
            model = yield maybe_future(contents_manager.get(path, content=False))
        except web.HTTPError as exc:
            if exc.status_code == 404 and "files" in path.split("/"):
                return FilesRedirectHandler.redirect_to_files(self, path)
            raise

        if model["type"] != "notebook":
            return FilesRedirectHandler.redirect_to_files(self, path)

        name = path.rsplit("/", 1)[-1]
        self.write(
            self.render_template(
                "notebook.html",
                notebook_path=path,
                notebook_name=name,
                kill_kernel=False,
                mathjax_url=self.mathjax_url,
                mathjax_config=self.mathjax_config,
                get_frontend_exporters=get_frontend_exporters,
            )
        )
        report_recent_path(path, self.log)


# NotebookApp 会先加载 extra_services，再注册默认 notebook handlers；相同路由
# 因而由本 Handler 优先处理，同时完整复用 Notebook 6.4.8 的页面和错误语义。
default_handlers = [
    (r"/notebooks%s" % path_regex, RecentNotebookHandler),
]
