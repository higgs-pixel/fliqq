"""Apply Fliq's Windows runner settings after `flutter create --platforms=windows .`.

- window title "Fliq", default size 1280x800, minimum size 960x640 (spec 10.4)
- long path support in the application manifest (spec 8.2)
Fails loudly if the Flutter template changed and an anchor is missing.
"""
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parent.parent / "windows" / "runner"


def patch(path, old, new, count=1):
    p = root / path
    s = p.read_text(encoding="utf-8")
    if new in s:
        return
    if s.count(old) != count:
        sys.exit(f"patch_windows_runner: anchor not found in {p}: {old!r}")
    p.write_text(s.replace(old, new), encoding="utf-8")


patch("main.cpp", "Win32Window::Size size(1280, 720);", "Win32Window::Size size(1280, 800);")
patch("main.cpp", 'window.Create(L"fliq", origin, size)', 'window.Create(L"Fliq", origin, size)')
patch(
    "win32_window.cpp",
    "  switch (message) {\n",
    "  switch (message) {\n"
    "    case WM_GETMINMAXINFO: {\n"
    "      auto info = reinterpret_cast<MINMAXINFO*>(lparam);\n"
    "      const double scale = FlutterDesktopGetDpiForHWnd(hwnd) / 96.0;\n"
    "      info->ptMinTrackSize.x = static_cast<LONG>(960 * scale);\n"
    "      info->ptMinTrackSize.y = static_cast<LONG>(640 * scale);\n"
    "      return 0;\n"
    "    }\n",
)
patch(
    "runner.exe.manifest",
    "<windowsSettings>",
    "<windowsSettings>\n      <longPathAware xmlns=\"http://schemas.microsoft.com/SMI/2016/WindowsSettings\">true</longPathAware>",
)
print("windows runner patched")
