"""Local Windows desktop MCP sidecar for BugleCat.

The server is intentionally process-isolated and exposes a small Win32 surface.
All coordinates are relative to the target window's client area.
"""

from __future__ import annotations

import ctypes
import json
import os
import struct
import tempfile
import time
from ctypes import wintypes
from pathlib import Path

from mcp.server.mcpserver import MCPServer
if os.name != "nt":
    raise RuntimeError("windows-computer-use only supports Windows")


user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
user32.GetForegroundWindow.restype = wintypes.HWND
user32.SetForegroundWindow.argtypes = [wintypes.HWND]
user32.BringWindowToTop.argtypes = [wintypes.HWND]
user32.IsWindow.argtypes = [wintypes.HWND]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.IsIconic.argtypes = [wintypes.HWND]
user32.IsZoomed.argtypes = [wintypes.HWND]
user32.GetDC.restype = wintypes.HDC
kernel32.OpenProcess.restype = wintypes.HANDLE
gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
gdi32.CreateCompatibleDC.restype = wintypes.HDC
gdi32.CreateCompatibleBitmap.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]
gdi32.CreateCompatibleBitmap.restype = wintypes.HBITMAP
gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]
gdi32.SelectObject.restype = wintypes.HGDIOBJ

SW_RESTORE = 9
SW_MAXIMIZE = 3
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
INPUT_MOUSE = 0
INPUT_KEYBOARD = 1
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
MOUSEEVENTF_RIGHTDOWN = 0x0008
MOUSEEVENTF_RIGHTUP = 0x0010
MOUSEEVENTF_WHEEL = 0x0800
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
SRCCOPY = 0x00CC0020
BI_RGB = 0
DIB_RGB_COLORS = 0


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    ]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    ]


class INPUT_UNION(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT)]


class INPUT(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("type", wintypes.DWORD), ("u", INPUT_UNION)]


server = MCPServer(
    "windows-computer-use",
    description="Local Windows window inspection and input control",
    instructions="Select a real window handle from list_windows before every workflow.",
)


def _check(ok: int | bool, operation: str) -> None:
    if not ok:
        raise ctypes.WinError(ctypes.get_last_error(), operation)


def _hwnd(handle: int) -> wintypes.HWND:
    value = int(handle)
    if not user32.IsWindow(value):
        raise ValueError(f"window handle is no longer valid: {value}")
    return wintypes.HWND(value)


def _handle_value(hwnd: wintypes.HWND | int) -> int:
    return int(hwnd.value) if isinstance(hwnd, ctypes.c_void_p) else int(hwnd)


def _window_text(hwnd: wintypes.HWND) -> str:
    length = user32.GetWindowTextLengthW(hwnd)
    buffer = ctypes.create_unicode_buffer(length + 1)
    user32.GetWindowTextW(hwnd, buffer, len(buffer))
    return buffer.value


def _process_path(pid: int) -> str:
    process = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not process:
        return ""
    try:
        size = wintypes.DWORD(32768)
        buffer = ctypes.create_unicode_buffer(size.value)
        if kernel32.QueryFullProcessImageNameW(process, 0, buffer, ctypes.byref(size)):
            return buffer.value
        return ""
    finally:
        kernel32.CloseHandle(process)


def _client_bounds(hwnd: wintypes.HWND) -> tuple[int, int, int, int]:
    rect = wintypes.RECT()
    _check(user32.GetClientRect(hwnd, ctypes.byref(rect)), "GetClientRect")
    origin = wintypes.POINT(0, 0)
    _check(user32.ClientToScreen(hwnd, ctypes.byref(origin)), "ClientToScreen")
    return origin.x, origin.y, origin.x + rect.right, origin.y + rect.bottom


def _capture_bmp(left: int, top: int, width: int, height: int, path: Path) -> None:
    """Capture a screen rectangle with Win32 GDI; requires no Python packages."""
    screen_dc = user32.GetDC(None)
    if not screen_dc:
        raise ctypes.WinError(ctypes.get_last_error(), "GetDC")
    memory_dc = gdi32.CreateCompatibleDC(screen_dc)
    bitmap = gdi32.CreateCompatibleBitmap(screen_dc, width, height)
    previous = None
    try:
        _check(memory_dc, "CreateCompatibleDC")
        _check(bitmap, "CreateCompatibleBitmap")
        previous = gdi32.SelectObject(memory_dc, bitmap)
        _check(previous, "SelectObject")
        _check(gdi32.BitBlt(memory_dc, 0, 0, width, height, screen_dc, left, top, SRCCOPY), "BitBlt")
        row_bytes = ((width * 24 + 31) // 32) * 4
        image_size = row_bytes * height
        info = BITMAPINFO(BITMAPINFOHEADER(
            ctypes.sizeof(BITMAPINFOHEADER), width, height, 1, 24, BI_RGB,
            image_size, 0, 0, 0, 0
        ))
        pixels = ctypes.create_string_buffer(image_size)
        _check(gdi32.GetDIBits(memory_dc, bitmap, 0, height, pixels, ctypes.byref(info), DIB_RGB_COLORS), "GetDIBits")
        file_header_size = 14
        pixel_offset = file_header_size + ctypes.sizeof(BITMAPINFOHEADER)
        with path.open("wb") as output:
            output.write(struct.pack("<2sIHHI", b"BM", pixel_offset + image_size, 0, 0, pixel_offset))
            output.write(bytes(info.bmiHeader))
            output.write(pixels.raw)
    finally:
        if previous:
            gdi32.SelectObject(memory_dc, previous)
        if bitmap:
            gdi32.DeleteObject(bitmap)
        if memory_dc:
            gdi32.DeleteDC(memory_dc)
        user32.ReleaseDC(None, screen_dc)


def _activate(hwnd: wintypes.HWND) -> None:
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, SW_RESTORE)
    foreground = user32.GetForegroundWindow()
    current_thread = kernel32.GetCurrentThreadId()
    foreground_thread = (
        user32.GetWindowThreadProcessId(foreground, None) if foreground else 0
    )
    attached = bool(
        foreground_thread
        and foreground_thread != current_thread
        and user32.AttachThreadInput(current_thread, foreground_thread, True)
    )
    try:
        user32.BringWindowToTop(hwnd)
        user32.SetForegroundWindow(hwnd)
    finally:
        if attached:
            user32.AttachThreadInput(current_thread, foreground_thread, False)
    time.sleep(0.12)
    if _handle_value(user32.GetForegroundWindow()) != _handle_value(hwnd):
        raise RuntimeError("Windows denied foreground activation; retry after selecting the window")


def _screen_point(hwnd: wintypes.HWND, x: int, y: int) -> tuple[int, int]:
    left, top, right, bottom = _client_bounds(hwnd)
    if x < 0 or y < 0 or left + x >= right or top + y >= bottom:
        raise ValueError(f"point ({x}, {y}) is outside client size {right-left}x{bottom-top}")
    return left + x, top + y


def _send(*inputs: INPUT) -> None:
    array = (INPUT * len(inputs))(*inputs)
    sent = user32.SendInput(len(inputs), array, ctypes.sizeof(INPUT))
    if sent != len(inputs):
        raise ctypes.WinError(ctypes.get_last_error(), "SendInput")


def _mouse(flags: int, data: int = 0) -> INPUT:
    item = INPUT(type=INPUT_MOUSE)
    item.mi = MOUSEINPUT(0, 0, data, flags, 0, 0)
    return item


def _key(vk: int, up: bool = False) -> INPUT:
    item = INPUT(type=INPUT_KEYBOARD)
    item.ki = KEYBDINPUT(vk, 0, KEYEVENTF_KEYUP if up else 0, 0, 0)
    return item


@server.tool(description="List visible top-level application windows. Read-only.")
def list_windows() -> str:
    windows: list[dict[str, object]] = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    @callback_type
    def callback(hwnd: wintypes.HWND, _lparam: wintypes.LPARAM) -> bool:
        if not user32.IsWindowVisible(hwnd):
            return True
        title = _window_text(hwnd).strip()
        if not title:
            return True
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        windows.append(
            {
                "handle": _handle_value(hwnd),
                "title": title,
                "pid": pid.value,
                "process_path": _process_path(pid.value),
                "foreground": int(user32.GetForegroundWindow()) == _handle_value(hwnd),
                "maximized": bool(user32.IsZoomed(hwnd)),
            }
        )
        return True

    _check(user32.EnumWindows(callback, 0), "EnumWindows")
    return json.dumps(windows, ensure_ascii=False)


@server.tool(description="Capture a window client area to a BMP and return its current geometry. Read-only.")
def get_window_state(handle: int) -> str:
    hwnd = _hwnd(handle)
    left, top, right, bottom = _client_bounds(hwnd)
    if right <= left or bottom <= top:
        raise ValueError("window client area is empty")
    output_dir = Path(tempfile.gettempdir()) / "buglecat-computer-use"
    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / f"window-{_handle_value(hwnd)}-{time.time_ns()}.bmp"
    _capture_bmp(left, top, right - left, bottom - top, path)
    return json.dumps(
        {
            "handle": _handle_value(hwnd),
            "title": _window_text(hwnd),
            "client_width": right - left,
            "client_height": bottom - top,
            "screenshot_path": str(path),
            "foreground": int(user32.GetForegroundWindow()) == _handle_value(hwnd),
            "maximized": bool(user32.IsZoomed(hwnd)),
        },
        ensure_ascii=False,
    )


@server.tool(description="Activate one selected application window.")
def activate_window(handle: int) -> str:
    hwnd = _hwnd(handle)
    _activate(hwnd)
    return json.dumps({"handle": _handle_value(hwnd), "activated": True})


@server.tool(description="Maximize one selected application window.")
def maximize_window(handle: int) -> str:
    hwnd = _hwnd(handle)
    user32.ShowWindow(hwnd, SW_MAXIMIZE)
    _activate(hwnd)
    return json.dumps({"handle": _handle_value(hwnd), "maximized": bool(user32.IsZoomed(hwnd))})


@server.tool(description="Click client-relative coordinates in one selected window.")
def click(handle: int, x: int, y: int, click_count: int = 1, button: str = "left") -> str:
    if click_count not in (1, 2):
        raise ValueError("click_count must be 1 or 2")
    hwnd = _hwnd(handle)
    _activate(hwnd)
    screen_x, screen_y = _screen_point(hwnd, int(x), int(y))
    _check(user32.SetCursorPos(screen_x, screen_y), "SetCursorPos")
    if button == "left":
        down, up = MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP
    elif button == "right":
        down, up = MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP
    else:
        raise ValueError("button must be 'left' or 'right'")
    for index in range(click_count):
        _send(_mouse(down), _mouse(up))
        if index + 1 < click_count:
            time.sleep(0.08)
    return json.dumps({"handle": _handle_value(hwnd), "x": x, "y": y, "click_count": click_count})


@server.tool(description="Type literal Unicode text into the focused control of one selected window.")
def type_text(handle: int, text: str) -> str:
    hwnd = _hwnd(handle)
    _activate(hwnd)
    for char in text:
        encoded = char.encode("utf-16-le")
        for offset in range(0, len(encoded), 2):
            scan = int.from_bytes(encoded[offset : offset + 2], "little")
            down = INPUT(type=INPUT_KEYBOARD)
            down.ki = KEYBDINPUT(0, scan, KEYEVENTF_UNICODE, 0, 0)
            up = INPUT(type=INPUT_KEYBOARD)
            up.ki = KEYBDINPUT(0, scan, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0, 0)
            _send(down, up)
    return json.dumps({"handle": _handle_value(hwnd), "typed_characters": len(text)})


@server.tool(description="Press a supported key or modifier chord in one selected window.")
def press_key(handle: int, key: str) -> str:
    hwnd = _hwnd(handle)
    _activate(hwnd)
    names = {
        "ctrl": 0x11,
        "control": 0x11,
        "alt": 0x12,
        "shift": 0x10,
        "enter": 0x0D,
        "return": 0x0D,
        "tab": 0x09,
        "escape": 0x1B,
        "esc": 0x1B,
        "space": 0x20,
        "backspace": 0x08,
        "delete": 0x2E,
        "up": 0x26,
        "down": 0x28,
        "left": 0x25,
        "right": 0x27,
    }
    parts = [part.strip().lower() for part in key.split("+") if part.strip()]
    if not parts or any(part in {"win", "windows", "meta", "super"} for part in parts):
        raise ValueError("empty key and Windows-key shortcuts are not allowed")
    codes: list[int] = []
    for part in parts:
        if part in names:
            codes.append(names[part])
        elif part.startswith("f") and part[1:].isdigit() and 1 <= int(part[1:]) <= 12:
            codes.append(0x70 + int(part[1:]) - 1)
        elif len(part) == 1 and part.isascii():
            codes.append(ord(part.upper()))
        else:
            raise ValueError(f"unsupported key: {part}")
    _send(*[_key(code) for code in codes], *[_key(code, True) for code in reversed(codes)])
    return json.dumps({"handle": _handle_value(hwnd), "key": key})


@server.tool(description="Scroll vertically at client-relative coordinates in one selected window.")
def scroll(handle: int, x: int, y: int, delta: int) -> str:
    hwnd = _hwnd(handle)
    _activate(hwnd)
    screen_x, screen_y = _screen_point(hwnd, int(x), int(y))
    _check(user32.SetCursorPos(screen_x, screen_y), "SetCursorPos")
    _send(_mouse(MOUSEEVENTF_WHEEL, ctypes.c_ulong(int(delta)).value))
    return json.dumps({"handle": _handle_value(hwnd), "x": x, "y": y, "delta": delta})


if __name__ == "__main__":
    server.run("stdio")
