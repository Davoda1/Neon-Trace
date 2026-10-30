#!/usr/bin/env python3
"""Native, bounded Windows acceptance capture for a disposable GitHub runner.

Usage: python tools/windows_ui_smoke.py --exe path/to/NeonTrace.exe --output captures
No third-party Python modules are needed. The app runs only its fictional --demo.
Only the runner's HKCU AppsUseLightTheme is changed, and its exact prior value is
restored in a finally block. This deliberately refuses to run on a user desktop.
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import json
import math
import os
import queue
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib

PERSONALIZE = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"
THEME_VALUE = "AppsUseLightTheme"


class NativeFailure(RuntimeError):
    """A short, public-safe acceptance failure code."""


class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


class POINT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", ctypes.c_long),
                ("biHeight", ctypes.c_long), ("biPlanes", wintypes.WORD),
                ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", ctypes.c_long),
                ("biYPelsPerMeter", ctypes.c_long), ("biClrUsed", wintypes.DWORD),
                ("biClrImportant", wintypes.DWORD)]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


def png_bytes(width: int, height: int, bgra: bytes) -> bytes:
    """Encode the GDI top-down BGRX bitmap as a portable RGB PNG."""
    if len(bgra) != width * height * 4:
        raise NativeFailure("capture_byte_count_invalid")
    rgb = bytearray(width * height * 3)
    rgb[0::3], rgb[1::3], rgb[2::3] = bgra[2::4], bgra[1::4], bgra[0::4]
    stride = width * 3
    scanlines = b"".join(b"\0" + rgb[y * stride:(y + 1) * stride]
                         for y in range(height))

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(scanlines, 6)) + chunk(b"IEND", b""))


def pixel_metrics(width: int, height: int, bgra: bytes) -> dict:
    # A blank or black PrintWindow capture must never count as a dark-theme pass.
    step = max(1, int(math.sqrt(width * height / 40000)))
    count = dark = light = blue = bright = 0
    total_luma = total_luma_sq = 0.0
    buckets: set[tuple[int, int, int]] = set()
    for y in range(0, height, step):
        for x in range(0, width, step):
            offset = (y * width + x) * 4
            b, g, r = bgra[offset:offset + 3]
            count += 1
            dark += max(r, g, b) < 75
            light += min(r, g, b) > 150
            blue += b > 80 and b > r + 40 and b > g + 15
            bright += max(r, g, b) > 110
            luma = 0.2126 * r + 0.7152 * g + 0.0722 * b
            total_luma += luma
            total_luma_sq += luma * luma
            buckets.add((r // 16, g // 16, b // 16))
    count = max(count, 1)
    mean = total_luma / count
    variance = max(0.0, total_luma_sq / count - mean * mean)
    result = {"dark_fraction": round(dark / count, 5), "light_fraction": round(light / count, 5),
              "blue_map_fraction": round(blue / count, 5), "bright_fraction": round(bright / count, 5),
              "mean_luminance": round(mean, 3), "luminance_variance": round(variance, 3),
              "color_buckets": len(buckets), "sample_count": count}
    result["rendered_content"] = len(buckets) >= 16 and blue / count > 0.001 and bright / count > 0.005 and variance > 40
    if dark / count > 0.45 and dark > light + count * 0.2:
        result["visible_theme"] = "dark"
    elif light / count > 0.45 and light > dark + count * 0.2:
        result["visible_theme"] = "light"
    else:
        result["visible_theme"] = "undetermined"
    return result


class Win32:
    def __init__(self) -> None:
        self.printwindow_timed_out = False
        self.reference_captures = {}
        self.user = ctypes.WinDLL("user32", use_last_error=True)
        self.gdi = ctypes.WinDLL("gdi32", use_last_error=True)
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self._bind(self.user, "FindWindowW", [wintypes.LPCWSTR, wintypes.LPCWSTR], wintypes.HWND)
        self._bind(self.user, "GetWindowThreadProcessId", [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)], wintypes.DWORD)
        self._bind(self.user, "GetClientRect", [wintypes.HWND, ctypes.POINTER(RECT)], wintypes.BOOL)
        self._bind(self.user, "GetWindowRect", [wintypes.HWND, ctypes.POINTER(RECT)], wintypes.BOOL)
        self._bind(self.user, "ClientToScreen", [wintypes.HWND, ctypes.POINTER(POINT)], wintypes.BOOL)
        self._bind(self.user, "ShowWindow", [wintypes.HWND, ctypes.c_int], wintypes.BOOL)
        self._bind(self.user, "SetForegroundWindow", [wintypes.HWND], wintypes.BOOL)
        self._bind(self.user, "BringWindowToTop", [wintypes.HWND], wintypes.BOOL)
        self._bind(self.user, "GetForegroundWindow", [], wintypes.HWND)
        self._bind(self.user, "SetWindowPos", [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.UINT], wintypes.BOOL)
        self._bind(self.user, "PostMessageW", [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM], wintypes.BOOL)
        self._bind(self.user, "SendMessageTimeoutW", [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM, wintypes.UINT, wintypes.UINT, ctypes.POINTER(ctypes.c_size_t)], ctypes.c_ssize_t)
        self._bind(self.user, "GetDC", [wintypes.HWND], wintypes.HDC)
        self._bind(self.user, "ReleaseDC", [wintypes.HWND, wintypes.HDC], ctypes.c_int)
        self._bind(self.user, "PrintWindow", [wintypes.HWND, wintypes.HDC, wintypes.UINT], wintypes.BOOL)
        self._bind(self.user, "GetSystemMetrics", [ctypes.c_int], ctypes.c_int)
        self._bind(self.gdi, "CreateCompatibleDC", [wintypes.HDC], wintypes.HDC)
        self._bind(self.gdi, "CreateCompatibleBitmap", [wintypes.HDC, ctypes.c_int, ctypes.c_int], wintypes.HBITMAP)
        self._bind(self.gdi, "SelectObject", [wintypes.HDC, wintypes.HGDIOBJ], wintypes.HGDIOBJ)
        self._bind(self.gdi, "DeleteObject", [wintypes.HGDIOBJ], wintypes.BOOL)
        self._bind(self.gdi, "DeleteDC", [wintypes.HDC], wintypes.BOOL)
        self._bind(self.gdi, "BitBlt", [wintypes.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.HDC, ctypes.c_int, ctypes.c_int, wintypes.DWORD], wintypes.BOOL)
        self._bind(self.gdi, "GetDIBits", [wintypes.HDC, wintypes.HBITMAP, wintypes.UINT, wintypes.UINT, wintypes.LPVOID, ctypes.POINTER(BITMAPINFO), wintypes.UINT], ctypes.c_int)
        if hasattr(self.user, "GetDpiForWindow"):
            self._bind(self.user, "GetDpiForWindow", [wintypes.HWND], wintypes.UINT)
        if hasattr(self.user, "SetThreadDpiAwarenessContext"):
            self._bind(self.user, "SetThreadDpiAwarenessContext", [ctypes.c_void_p], ctypes.c_void_p)
            self.user.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2

    @staticmethod
    def _bind(dll, name, arguments, result) -> None:
        function = getattr(dll, name)
        function.argtypes, function.restype = arguments, result

    def dpi(self, hwnd) -> int:
        return int(self.user.GetDpiForWindow(hwnd)) if hasattr(self.user, "GetDpiForWindow") else 96

    def bounds(self, hwnd) -> dict:
        client, outer = RECT(), RECT()
        if not self.user.GetClientRect(hwnd, ctypes.byref(client)) or not self.user.GetWindowRect(hwnd, ctypes.byref(outer)):
            raise NativeFailure("window_bounds_unavailable")
        dpi = self.dpi(hwnd)
        scale = max(dpi / 96.0, 1.0)
        return {"client_width": client.right - client.left, "client_height": client.bottom - client.top,
                "window_width": outer.right - outer.left, "window_height": outer.bottom - outer.top,
                "logical_client_width": round((client.right - client.left) / scale, 2),
                "logical_client_height": round((client.bottom - client.top) / scale, 2), "dpi": dpi}

    def resize(self, hwnd, logical_width: int, logical_height: int) -> None:
        before = self.bounds(hwnd)
        scale = before["dpi"] / 96.0
        width = round(logical_width * scale) + before["window_width"] - before["client_width"]
        height = round(logical_height * scale) + before["window_height"] - before["client_height"]
        if not self.user.SetWindowPos(hwnd, None, 16, 16, width, height, 0x0040):  # SWP_SHOWWINDOW
            raise NativeFailure("window_resize_failed")
        self.user.ShowWindow(hwnd, 9)  # SW_RESTORE
        self.user.BringWindowToTop(hwnd)
        self.user.SetForegroundWindow(hwnd)

    def find(self, pid: int):
        hwnd = self.user.FindWindowW(None, "NeonTrace")
        if hwnd:
            found_pid = wintypes.DWORD()
            self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(found_pid))
            if found_pid.value == pid:
                return hwnd
        return None

    def broadcast_theme(self) -> None:
        name = ctypes.create_unicode_buffer("ImmersiveColorSet")
        result = ctypes.c_size_t()
        self.user.SendMessageTimeoutW(wintypes.HWND(0xFFFF), 0x001A, 0, ctypes.cast(name, ctypes.c_void_p).value,
                                      0x0002, 2000, ctypes.byref(result))

    def capture(self, hwnd, source: str) -> tuple[int, int, bytes]:
        if hasattr(self.user, "SetThreadDpiAwarenessContext"):
            self.user.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))
        bounds = self.bounds(hwnd)
        width, height = bounds["client_width"], bounds["client_height"]
        if not 100 <= width <= 8192 or not 100 <= height <= 8192:
            raise NativeFailure("capture_dimensions_invalid")
        desktop = self.user.GetDC(None)
        memory = bitmap = previous = None
        try:
            if not desktop:
                raise NativeFailure("capture_device_context_unavailable")
            memory = self.gdi.CreateCompatibleDC(desktop)
            bitmap = self.gdi.CreateCompatibleBitmap(desktop, width, height)
            if not memory or not bitmap:
                raise NativeFailure("capture_bitmap_unavailable")
            previous = self.gdi.SelectObject(memory, bitmap)
            if source == "printwindow":
                ok = self.user.PrintWindow(hwnd, memory, 0x00000003)  # CLIENTONLY | RENDERFULLCONTENT
            elif source == "desktop_bitblt":
                # Capture only this foreground app's client rectangle; never the desktop.
                point = POINT(0, 0)
                self.user.ClientToScreen(hwnd, ctypes.byref(point))
                vx, vy = self.user.GetSystemMetrics(76), self.user.GetSystemMetrics(77)
                vw, vh = self.user.GetSystemMetrics(78), self.user.GetSystemMetrics(79)
                if self.user.GetForegroundWindow() != hwnd or not (vx <= point.x and vy <= point.y and point.x + width <= vx + vw and point.y + height <= vy + vh):
                    raise NativeFailure("foreground_client_not_fully_visible")
                ok = self.gdi.BitBlt(memory, 0, 0, width, height, desktop, point.x, point.y, 0x00CC0020 | 0x40000000)
            else:
                raise NativeFailure("unknown_capture_source")
            if not ok:
                raise NativeFailure("capture_api_failed")
            self.gdi.SelectObject(memory, previous)
            previous = None
            info = BITMAPINFO()
            info.bmiHeader = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), width, -height, 1, 32, 0, width * height * 4, 0, 0, 0, 0)
            pixels = ctypes.create_string_buffer(width * height * 4)
            if self.gdi.GetDIBits(desktop, bitmap, 0, height, pixels, ctypes.byref(info), 0) != height:
                raise NativeFailure("capture_pixel_read_failed")
            return width, height, pixels.raw
        finally:
            if previous and memory:
                self.gdi.SelectObject(memory, previous)
            if bitmap:
                self.gdi.DeleteObject(bitmap)
            if memory:
                self.gdi.DeleteDC(memory)
            if desktop:
                self.user.ReleaseDC(None, desktop)


class RunnerTheme:
    def __init__(self, api: Win32):
        import winreg
        self.reg, self.api = winreg, api
        self.key_existed = True
        try:
            key = winreg.OpenKey(winreg.HKEY_CURRENT_USER, PERSONALIZE, 0, winreg.KEY_QUERY_VALUE)
        except FileNotFoundError:
            key, self.key_existed = None, False
        self.original = None
        if key:
            with key:
                try:
                    self.original = winreg.QueryValueEx(key, THEME_VALUE)
                except FileNotFoundError:
                    pass
        self.changed = False

    def set(self, light: bool) -> None:
        with self.reg.CreateKeyEx(self.reg.HKEY_CURRENT_USER, PERSONALIZE, 0, self.reg.KEY_SET_VALUE) as key:
            self.reg.SetValueEx(key, THEME_VALUE, 0, self.reg.REG_DWORD, int(light))
        self.changed = True
        self.api.broadcast_theme()

    def restore(self) -> bool:
        if not self.changed:
            return True
        with self.reg.OpenKey(self.reg.HKEY_CURRENT_USER, PERSONALIZE, 0, self.reg.KEY_SET_VALUE | self.reg.KEY_QUERY_VALUE) as key:
            if self.original is None:
                try:
                    self.reg.DeleteValue(key, THEME_VALUE)
                except FileNotFoundError:
                    pass
            else:
                self.reg.SetValueEx(key, THEME_VALUE, 0, self.original[1], self.original[0])
        # Preserve any other values/keys the runner already owns.
        self.api.broadcast_theme()
        with self.reg.OpenKey(self.reg.HKEY_CURRENT_USER, PERSONALIZE, 0, self.reg.KEY_QUERY_VALUE) as key:
            try:
                restored = self.reg.QueryValueEx(key, THEME_VALUE)
            except FileNotFoundError:
                restored = None
        return restored == self.original


def wait_window(api: Win32, process: subprocess.Popen, timeout: float):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise NativeFailure("application_exited_before_window")
        hwnd = api.find(process.pid)
        if hwnd:
            return hwnd
        time.sleep(0.1)
    raise NativeFailure("application_window_timeout")


def bounded_capture(api: Win32, hwnd, source: str) -> tuple[int, int, bytes]:
    if source != "printwindow":
        return api.capture(hwnd, source)
    if api.printwindow_timed_out:
        raise NativeFailure("printwindow_previously_timed_out")
    # PrintWindow sends synchronous paint messages and can hang on a broken UI.
    # Give one daemon worker ownership of its GDI resources; do not free its DC
    # from another thread. A timeout disables this source for the rest of the run.
    results: queue.Queue = queue.Queue(maxsize=1)
    def capture_worker():
        try:
            results.put((True, api.capture(hwnd, source)))
        except Exception as error:
            results.put((False, error))
    threading.Thread(target=capture_worker, daemon=True).start()
    try:
        ok, value = results.get(timeout=2.0)
    except queue.Empty:
        api.printwindow_timed_out = True
        raise NativeFailure("printwindow_capture_timeout") from None
    if not ok:
        raise value
    return value


def prior_wide_crop_match(width: int, height: int, pixels: bytes, reference) -> float | None:
    """Reject a resized GDI surface that is merely the old wide bitmap's crop."""
    if reference is None:
        return None
    old_width, old_height, old_pixels = reference
    if old_width <= width + 20 or old_height < height:
        return None
    step = max(1, int(math.sqrt(width * height / 20000)))
    matches = count = 0
    for y in range(0, height, step):
        for x in range(0, width, step):
            current = (y * width + x) * 4
            previous = (y * old_width + x) * 4
            # Alpha from a GDI surface is undefined; compare actual RGB only.
            matches += pixels[current:current + 3] == old_pixels[previous:previous + 3]
            count += 1
    return matches / max(count, 1)


def capture_phase(api: Win32, hwnd, process: subprocess.Popen, output: Path,
                  name: str, expected_theme: str, timeout: float) -> dict:
    deadline, started = time.monotonic() + timeout, time.monotonic()
    stable, previous_source = 0, None
    last_image = last_metrics = None
    errors: set[str] = set()
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise NativeFailure("application_exited_during_capture")
        # GetDC(hwnd)/client BitBlt may return the old GPU backing bitmap after
        # resize. It is never accepted as evidence. Use PrintWindow or the
        # validated, fully visible foreground client rectangle on the desktop.
        for source in ("printwindow", "desktop_bitblt"):
            try:
                width, height, pixels = bounded_capture(api, hwnd, source)
            except NativeFailure as error:
                errors.add(str(error))
                continue
            metrics = pixel_metrics(width, height, pixels)
            crop_match = prior_wide_crop_match(width, height, pixels, api.reference_captures.get(expected_theme)) if name.startswith("minimum-") else None
            if crop_match is not None:
                metrics["prior_wide_crop_match_fraction"] = round(crop_match, 6)
                metrics["stale_wide_crop_rejected"] = crop_match >= 0.985
            last_image, last_metrics = (width, height, pixels), metrics
            if metrics.get("stale_wide_crop_rejected"):
                errors.add("stale_prior_wide_bitmap_crop")
                stable, previous_source = 0, None
                continue
            if metrics["rendered_content"] and metrics["visible_theme"] == expected_theme:
                stable = stable + 1 if previous_source == source else 1
                previous_source = source
                if stable >= 2:
                    filename = name + ".png"
                    (output / filename).write_bytes(png_bytes(width, height, pixels))
                    if not name.startswith("minimum-"):
                        api.reference_captures[expected_theme] = (width, height, pixels)
                    return {"phase": name, "passed": True, "expected_theme": expected_theme,
                            "screenshot": filename, "capture_source": source, "bounds": api.bounds(hwnd),
                            "elapsed_seconds": round(time.monotonic() - started, 3), "metrics": metrics}
                break
        time.sleep(0.35)
    if last_image:
        (output / (name + "-last.png")).write_bytes(png_bytes(*last_image))
    return {"phase": name, "passed": False, "expected_theme": expected_theme,
            "elapsed_seconds": round(time.monotonic() - started, 3), "bounds": api.bounds(hwnd),
            "metrics": last_metrics, "capture_errors": sorted(errors), "error": "visible_theme_or_content_timeout"}


def run(arguments) -> int:
    if os.name != "nt" or os.environ.get("GITHUB_ACTIONS", "").lower() != "true" or os.environ.get("RUNNER_OS") != "Windows":
        print("This acceptance script runs only on a disposable GitHub Actions Windows runner.", file=sys.stderr)
        return 2
    executable = Path(arguments.exe).resolve()
    if not executable.is_file():
        print("The NeonTrace executable is missing.", file=sys.stderr)
        return 2
    output = Path(arguments.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    timeout = min(30.0, max(5.0, arguments.phase_timeout))
    api = Win32()
    theme = RunnerTheme(api)
    report = {"schema_version": 1, "platform": "native_windows", "executable": executable.name,
              "mode": "offline_fictional_demo", "phase_timeout_seconds": timeout, "phases": [],
              "passed": False, "registry_restored": False, "clean_close": False,
              "scope": "Pixel checks verify visible native rendering and OS theme changes; screenshots still require layout review."}
    process = hwnd = None
    isolated_profile = tempfile.TemporaryDirectory(prefix="NeonTrace-native-smoke-")
    isolated = isolated_profile.name
    try:
        base = Path(isolated)
        roaming, local = base / "roaming", base / "local"
        roaming.mkdir()
        local.mkdir()
        environment = os.environ.copy()
        environment.update(APPDATA=str(roaming), LOCALAPPDATA=str(local))
        theme.set(False)
        process = subprocess.Popen([str(executable), "--demo"], env=environment, cwd=isolated,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        report["process_id"] = process.pid
        hwnd = wait_window(api, process, timeout)
        screen_width, screen_height = api.user.GetSystemMetrics(0), api.user.GetSystemMetrics(1)
        scale = api.dpi(hwnd) / 96.0
        comfortable_width = max(680, min(1280, int((screen_width - 48) / scale)))
        comfortable_height = max(540, min(800, int((screen_height - 96) / scale)))
        report["screen"] = {"width": screen_width, "height": screen_height}
        api.resize(hwnd, comfortable_width, comfortable_height)
        phases = [("startup-dark", "dark", None), ("runtime-light", "light", True),
                  ("runtime-dark", "dark", False)]
        for name, expected, light in phases:
            if light is not None:
                theme.set(light)
            result = capture_phase(api, hwnd, process, output, name, expected, timeout)
            report["phases"].append(result)
            if not result["passed"]:
                raise NativeFailure(name + "_failed")
        # Request precisely 680x540 logical client units; the native minimum
        # must remain usable and must not be confused with frame dimensions.
        api.resize(hwnd, 680, 540)
        result = capture_phase(api, hwnd, process, output, "minimum-dark", "dark", timeout)
        bounds = result["bounds"]
        result["minimum_size_verified"] = abs(bounds["logical_client_width"] - 680) <= 3 and abs(bounds["logical_client_height"] - 540) <= 3
        result["passed"] = result["passed"] and result["minimum_size_verified"]
        report["phases"].append(result)
        if not result["passed"]:
            raise NativeFailure("minimum_dark_failed")
        theme.set(True)
        result = capture_phase(api, hwnd, process, output, "minimum-light", "light", timeout)
        report["phases"].append(result)
        if not result["passed"]:
            raise NativeFailure("minimum_light_failed")
        dpi = api.dpi(hwnd)
        report["high_dpi"] = {"actual_window_dpi": dpi, "tested": dpi > 96,
                              "status": "native_scaled_monitor" if dpi > 96 else "not_available_on_runner_96dpi",
                              "note": "No artificial DPI result is claimed; changing monitor scale requires a supported desktop session."}
        # Demo must not install downloaded data or opt into persisted history.
        report["isolated_data"] = {"history_files": len(list(local.glob("NeonTrace/history/*.json"))),
                                   "mmdb_files": len(list(local.rglob("*.mmdb")))}
        if report["isolated_data"]["history_files"] or report["isolated_data"]["mmdb_files"]:
            raise NativeFailure("demo_created_history_or_geodata")
        if not api.user.PostMessageW(hwnd, 0x0010, 0, 0):  # WM_CLOSE
            raise NativeFailure("close_message_failed")
        process.wait(timeout=10)
        report["clean_close"] = process.returncode == 0
        report["exit_code"] = process.returncode
        if not report["clean_close"]:
            raise NativeFailure("application_exit_nonzero")
        settings_file = roaming / "NeonTrace" / "settings.json"
        report["isolated_config_verified"] = settings_file.is_file()
        if not report["isolated_config_verified"]:
            raise NativeFailure("isolated_appdata_profile_was_not_used")
        saved_settings = json.loads(settings_file.read_text(encoding="utf-8"))
        if saved_settings.get("history_enabled") is not False:
            raise NativeFailure("demo_history_was_enabled")
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error) if isinstance(error, NativeFailure) else type(error).__name__
    finally:
        try:
            if process is not None and process.poll() is None:
                if hwnd:
                    api.user.PostMessageW(hwnd, 0x0010, 0, 0)
                try:
                    process.wait(timeout=5)
                    report["clean_close"] = process.returncode == 0
                except subprocess.TimeoutExpired:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)
                    report["forced_termination"] = True
        except Exception as error:
            report["process_cleanup_error"] = type(error).__name__
        try:
            report["registry_restored"] = theme.restore()
        except Exception as error:
            report["registry_restore_error"] = type(error).__name__
        try:
            isolated_profile.cleanup()
            report["isolated_profile_removed"] = True
        except Exception as error:
            report["isolated_profile_removed"] = False
            report["isolated_profile_cleanup_error"] = type(error).__name__
        report["passed"] = report["passed"] and report["registry_restored"] and report["clean_close"] and report["isolated_profile_removed"]
        (output / "windows-native-acceptance.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": report["passed"], "phases": len(report["phases"]),
                      "registry_restored": report["registry_restored"], "clean_close": report["clean_close"],
                      "report": "windows-native-acceptance.json", "error": report.get("error")}, indent=2))
    return 0 if report["passed"] else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, help="The built native NeonTrace.exe")
    parser.add_argument("--output", default="windows-native-acceptance", help="JSON/PNG artifact directory")
    parser.add_argument("--phase-timeout", type=float, default=30, help="Per-phase timeout, capped at30seconds")
    return run(parser.parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
