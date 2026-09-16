"""Compare Python winreg app list vs Rust list_apps JSON. Read-only."""

from __future__ import annotations

import json
import subprocess
import sys
import winreg


def python_list() -> list[str]:
    names: set[str] = set()
    sources = [
        (winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", winreg.KEY_WOW64_64KEY),
        (winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", winreg.KEY_WOW64_32KEY),
        (winreg.HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", winreg.KEY_WOW64_64KEY),
    ]
    for hive, sub, access in sources:
        try:
            root = winreg.OpenKey(hive, sub, 0, winreg.KEY_READ | access)
        except OSError:
            continue
        i = 0
        while True:
            try:
                name = winreg.EnumKey(root, i)
            except OSError:
                break
            i += 1
            try:
                item = winreg.OpenKey(root, name, 0, winreg.KEY_READ | access)
            except OSError:
                continue
            try:
                display, _ = winreg.QueryValueEx(item, "DisplayName")
                uninst, _ = winreg.QueryValueEx(item, "UninstallString")
            except OSError:
                continue
            finally:
                winreg.CloseKey(item)
            if display and uninst:
                names.add(str(display).strip().lower())
        winreg.CloseKey(root)
    return sorted(names)


def rust_list() -> list[str]:
    exe = r"D:\Agent-Project\XiaomiMiMoProjects\Remova-next\src-tauri\target\debug\list_apps.exe"
    out = subprocess.check_output([exe], text=True, encoding="utf-8")
    data = json.loads(out)
    return sorted({str(a["name"]).strip().lower() for a in data if a.get("name")})


def main() -> int:
    p = set(python_list())
    r = set(rust_list())
    only_p = sorted(p - r)
    only_r = sorted(r - p)
    both = p & r
    print(f"python={len(p)} rust={len(r)} intersection={len(both)}")
    print(f"only_python={len(only_p)} only_rust={len(only_r)}")
    print("--- only python (first 20) ---")
    for n in only_p[:20]:
        print(n)
    print("--- only rust (first 20) ---")
    for n in only_r[:20]:
        print(n)
    # Accept if intersection is large relative to python (heuristic)
    if not p:
        print("FAIL: python list empty")
        return 1
    ratio = len(both) / len(p)
    print(f"coverage={ratio:.1%}")
    return 0 if ratio >= 0.85 else 2


if __name__ == "__main__":
    raise SystemExit(main())
