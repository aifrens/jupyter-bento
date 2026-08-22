#!/usr/bin/env python3
"""Verify that make-dmg writes lossless Python settings without executing paths."""

from __future__ import annotations

import json
import os
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
MAKE_DMG = REPO_ROOT / "runtime" / "make-dmg.sh"
VOLUME_NAME = "朱比特和它的朋友们"


def write_executable(path: Path, content: str) -> None:
    path.write_text(textwrap.dedent(content).lstrip(), encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


class MakeDmgSettingsTest(unittest.TestCase):
    def test_paths_are_serialized_without_executing_them(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            fake_bin = root / "fake-bin"
            fake_bin.mkdir()
            fake_venv_python = root / "fake-venv-python"
            fake_dmgbuild = root / "fake-dmgbuild"
            capture_script = root / "capture-settings.py"

            write_executable(
                fake_bin / "python3",
                """
                #!/bin/sh
                set -eu
                if [ "$#" -eq 2 ] && [ "$1" = "-c" ]; then
                    printf '3.13.5\n'
                    exit 0
                fi
                if [ "$#" -eq 3 ] && [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
                    mkdir -p "$3/bin"
                    cp "$FAKE_VENV_PYTHON" "$3/bin/python"
                    cp "$FAKE_DMGBUILD" "$3/bin/dmgbuild"
                    exit 0
                fi
                exec "$REAL_PYTHON" "$@"
                """,
            )
            write_executable(
                fake_venv_python,
                """
                #!/bin/sh
                set -eu
                args=" $* "
                case "$args" in *" -m pip install "*) ;; *) exit 64 ;; esac
                case "$args" in *" --require-hashes "*) ;; *) exit 65 ;; esac
                case "$args" in *" --only-binary=:all: "*) ;; *) exit 66 ;; esac
                case "$args" in *" --no-deps "*) ;; *) exit 67 ;; esac
                case "$args" in *" -r "*"requirements-dmgbuild.lock.txt"*) exit 0 ;; esac
                exit 68
                """,
            )
            write_executable(
                fake_dmgbuild,
                """
                #!/bin/sh
                set -eu
                exec "$REAL_PYTHON" "$DMG_CAPTURE_SCRIPT" "$@"
                """,
            )
            capture_script.write_text(
                textwrap.dedent(
                    """
                    import json
                    import os
                    import runpy
                    import sys
                    from pathlib import Path

                    if len(sys.argv) != 5 or sys.argv[1] != "-s":
                        raise SystemExit(f"unexpected dmgbuild arguments: {sys.argv[1:]!r}")
                    settings_path = Path(sys.argv[2])
                    namespace = runpy.run_path(str(settings_path))
                    payload = {
                        "files": namespace["files"],
                        "symlinks": namespace["symlinks"],
                        "background": namespace["background"],
                        "window_rect": namespace["window_rect"],
                        "icon_size": namespace["icon_size"],
                        "icon_locations": namespace["icon_locations"],
                        "format": namespace["format"],
                        "volume_name": sys.argv[3],
                    }
                    Path(os.environ["DMG_CAPTURE_PATH"]).write_text(
                        json.dumps(payload, ensure_ascii=False), encoding="utf-8"
                    )
                    Path(sys.argv[4]).write_bytes(b"fake dmg")
                    """
                ).lstrip(),
                encoding="utf-8",
            )

            base_environment = os.environ.copy()
            base_environment.update(
                {
                    "PATH": f"{fake_bin}{os.pathsep}{base_environment['PATH']}",
                    "REAL_PYTHON": sys.executable,
                    "FAKE_VENV_PYTHON": str(fake_venv_python),
                    "FAKE_DMGBUILD": str(fake_dmgbuild),
                    "DMG_CAPTURE_SCRIPT": str(capture_script),
                }
            )

            cases = {
                "ordinary": Path("Apps") / "朱比特和它的朋友们.app",
                "quotes": Path("Apps") / "double\" and single'.app",
                "backslash": Path("Apps") / "back\\slash.app",
                "newline": Path("Apps") / "line\nbreak.app",
                "injection": (
                    Path(
                        'escape")]; __import__("pathlib").Path("owned")'
                        '.write_text("owned"); #'
                    )
                    / "ordinary.app"
                ),
            }

            for label, relative_app_path in cases.items():
                with self.subTest(label=label):
                    case_root = root / label
                    case_root.mkdir()
                    app_path = case_root / relative_app_path
                    app_path.mkdir(parents=True)
                    output_path = case_root / "dist" / f"{label}.dmg"
                    capture_path = case_root / "capture.json"
                    environment = base_environment | {
                        "DMG_CAPTURE_PATH": str(capture_path)
                    }

                    result = subprocess.run(
                        [str(MAKE_DMG), str(app_path), str(output_path)],
                        cwd=case_root,
                        env=environment,
                        capture_output=True,
                        text=True,
                        check=False,
                    )

                    self.assertEqual(
                        result.returncode,
                        0,
                        msg=f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
                    )
                    self.assertFalse(
                        (case_root / "owned").exists(),
                        "the generated settings executed data from APP_PATH",
                    )
                    captured = json.loads(capture_path.read_text(encoding="utf-8"))
                    app_name = app_path.name
                    self.assertEqual(captured["files"], [[str(app_path), app_name]])
                    self.assertEqual(captured["symlinks"], {"应用程序": "/Applications"})
                    self.assertEqual(
                        captured["background"],
                        str(REPO_ROOT / "runtime" / "dmg-background.png"),
                    )
                    self.assertEqual(captured["window_rect"], [[300, 120], [640, 400]])
                    self.assertEqual(captured["icon_size"], 100)
                    self.assertEqual(
                        captured["icon_locations"],
                        {app_name: [160, 170], "应用程序": [480, 170]},
                    )
                    self.assertEqual(captured["format"], "UDZO")
                    self.assertEqual(captured["volume_name"], VOLUME_NAME)
                    self.assertEqual(output_path.read_bytes(), b"fake dmg")


if __name__ == "__main__":
    unittest.main()
