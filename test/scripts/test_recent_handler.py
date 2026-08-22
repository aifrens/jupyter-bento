#!/usr/bin/env python3
"""验证最近文件 Handler 的路由契约与回调请求。"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import types
import unittest
from pathlib import Path
from unittest import mock


# 测试直接从应用资源加载模块，不在资源目录留下构建外的 .pyc 文件。
sys.dont_write_bytecode = True


ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = (
    ROOT
    / "app"
    / "src-tauri"
    / "resources"
    / "jupiter_recent_handler"
    / "handlers.py"
)


def load_handler_module():
    """用 Notebook 6.4.8 形状的最小模块桩加载产品 Handler。"""
    notebook = types.ModuleType("notebook")
    notebook.__path__ = []
    notebook_base = types.ModuleType("notebook.base")
    notebook_base.__path__ = []
    notebook_base_handlers = types.ModuleType("notebook.base.handlers")
    notebook_base_handlers.path_regex = r"(?P<path>.*)"

    class FilesRedirectHandler:
        @staticmethod
        def redirect_to_files(handler, path):
            return handler.redirected_paths.append(path)

    notebook_base_handlers.FilesRedirectHandler = FilesRedirectHandler
    notebook_notebook = types.ModuleType("notebook.notebook")
    notebook_notebook.__path__ = []
    notebook_notebook_handlers = types.ModuleType("notebook.notebook.handlers")

    class NotebookHandler:
        def get(self, path):
            return None

    notebook_notebook_handlers.NotebookHandler = NotebookHandler
    notebook_notebook_handlers.get_frontend_exporters = mock.sentinel.exporters
    notebook_utils = types.ModuleType("notebook.utils")
    notebook_utils.maybe_future = lambda value: value

    tornado = types.ModuleType("tornado")
    tornado_gen = types.ModuleType("tornado.gen")
    tornado_gen.coroutine = lambda function: function
    tornado_web = types.ModuleType("tornado.web")
    tornado_web.authenticated = lambda function: function

    class HTTPError(Exception):
        def __init__(self, status_code):
            super().__init__(status_code)
            self.status_code = status_code

    tornado_web.HTTPError = HTTPError
    tornado.gen = tornado_gen
    tornado.web = tornado_web

    stubs = {
        "notebook": notebook,
        "notebook.base": notebook_base,
        "notebook.base.handlers": notebook_base_handlers,
        "notebook.notebook": notebook_notebook,
        "notebook.notebook.handlers": notebook_notebook_handlers,
        "notebook.utils": notebook_utils,
        "tornado": tornado,
        "tornado.gen": tornado_gen,
        "tornado.web": tornado_web,
    }
    with mock.patch.dict(sys.modules, stubs):
        spec = importlib.util.spec_from_file_location("recent_handler_under_test", MODULE_PATH)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
    return module


class RecentHandlerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.module = load_handler_module()

    def test_registers_before_default_notebook_handler_via_matching_route(self):
        route, handler = self.module.default_handlers[0]
        self.assertEqual(route, r"/notebooks(?P<path>.*)")
        self.assertIs(handler, self.module.RecentNotebookHandler)

    def test_posts_relative_unicode_path_and_bearer_token(self):
        relative_path = "分析/季度 报告.ipynb"
        response = mock.Mock()
        with mock.patch.object(
            self.module,
            "urlopen",
            return_value=response,
        ) as urlopen:
            self.module._post_recent_path(
                "http://127.0.0.1:32145/recent",
                "callback-secret",
                relative_path,
            )

        request = urlopen.call_args.args[0]
        self.assertEqual(request.full_url, "http://127.0.0.1:32145/recent")
        self.assertEqual(request.get_header("Authorization"), "Bearer callback-secret")
        self.assertEqual(
            request.get_header("Content-type"), "application/json; charset=utf-8"
        )
        self.assertEqual(
            json.loads(request.data.decode("utf-8")),
            {"path": relative_path},
        )
        urlopen.assert_called_once_with(request, timeout=1.0)
        response.read.assert_called_once_with()
        response.close.assert_called_once_with()

    def test_missing_callback_config_does_not_start_worker(self):
        with mock.patch.dict(
            os.environ,
            {
                self.module.CALLBACK_URL_ENV: "",
                self.module.CALLBACK_TOKEN_ENV: "",
            },
            clear=False,
        ), mock.patch.object(self.module.threading, "Thread") as thread:
            self.module.report_recent_path("notebook.ipynb")
        thread.assert_not_called()

    def make_handler(self, model):
        handler = object.__new__(self.module.RecentNotebookHandler)
        handler.log = mock.Mock()
        handler.contents_manager = mock.Mock()
        handler.contents_manager.get.return_value = model
        handler.redirected_paths = []
        handler.mathjax_url = "mathjax-url"
        handler.mathjax_config = "mathjax-config"
        handler.render_template = mock.Mock(return_value="rendered notebook")
        handler.write = mock.Mock()
        return handler

    def finish_get(self, generator, yielded_model):
        """模拟 Tornado coroutine runner 把 yield 的结果送回生成器。"""
        with self.assertRaises(StopIteration):
            generator.send(yielded_model)

    def test_reports_only_after_notebook_is_rendered(self):
        handler = self.make_handler({"type": "notebook"})
        with mock.patch.object(self.module, "report_recent_path") as report:
            generator = self.module.RecentNotebookHandler.get(
                handler, "/folder/notebook.ipynb/"
            )
            self.assertEqual(next(generator), {"type": "notebook"})
            self.finish_get(generator, {"type": "notebook"})
        handler.contents_manager.get.assert_called_once_with(
            "folder/notebook.ipynb", content=False
        )
        handler.render_template.assert_called_once_with(
            "notebook.html",
            notebook_path="folder/notebook.ipynb",
            notebook_name="notebook.ipynb",
            kill_kernel=False,
            mathjax_url="mathjax-url",
            mathjax_config="mathjax-config",
            get_frontend_exporters=mock.sentinel.exporters,
        )
        handler.write.assert_called_once_with("rendered notebook")
        report.assert_called_once_with("folder/notebook.ipynb", handler.log)

    def test_non_notebook_redirect_is_not_reported(self):
        handler = self.make_handler({"type": "file"})
        with mock.patch.object(self.module, "report_recent_path") as report:
            generator = self.module.RecentNotebookHandler.get(handler, "notes.txt")
            self.assertEqual(next(generator), {"type": "file"})
            self.finish_get(generator, {"type": "file"})
        self.assertEqual(handler.redirected_paths, ["notes.txt"])
        handler.render_template.assert_not_called()
        report.assert_not_called()

    def test_missing_notebook_is_not_reported(self):
        handler = self.make_handler(None)
        handler.contents_manager.get.side_effect = self.module.web.HTTPError(404)
        with mock.patch.object(self.module, "report_recent_path") as report:
            generator = self.module.RecentNotebookHandler.get(handler, "missing.ipynb")
            with self.assertRaises(self.module.web.HTTPError):
                next(generator)
        report.assert_not_called()


if __name__ == "__main__":
    unittest.main()
