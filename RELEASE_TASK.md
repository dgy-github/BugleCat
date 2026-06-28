# Release task — package & upload the nanocodex GUI installer

Hand this file to a coding agent (e.g. codex) to publish the Windows installer as a
GitHub Release. **Self-contained.** Use absolute paths; do **not** switch branches or
touch the working tree (the code is already committed and pushed).

| Field | Value |
|---|---|
| Repo | `dgy-github/nanocodex` |
| Branch (release target) | `feat/gui` |
| Tag | `gui-v0.1.0` |
| Installer | `D:\agent_prac\ncx-gui\rust\gui\src-tauri\target\release\bundle\nsis\nanocodex_0.1.0_x64-setup.exe` |
| Env | Windows / git-bash; cargo at `~/.cargo/bin` |

## Steps

### 1. Ensure `gh` is authenticated
```bash
gh auth status
```
If not logged in: run `gh auth login` (GitHub.com → HTTPS → web browser) **or** set
`GH_TOKEN`, then continue. (Auth is a hard prerequisite — without it, stop here.)

### 2. Ensure the installer exists; rebuild only if missing
```bash
PKG="D:/agent_prac/ncx-gui/rust/gui/src-tauri/target/release/bundle/nsis/nanocodex_0.1.0_x64-setup.exe"
if [ ! -f "$PKG" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
  ( cd /d/agent_prac/ncx-gui/rust/gui && cargo tauri build )
fi
ls -la "$PKG"
```

### 3. Create the release and upload the installer
Creates the tag at `feat/gui` HEAD and attaches the installer as an asset:
```bash
gh release create gui-v0.1.0 \
  "D:/agent_prac/ncx-gui/rust/gui/src-tauri/target/release/bundle/nsis/nanocodex_0.1.0_x64-setup.exe#nanocodex 0.1.0 安装包 (Windows x64)" \
  --repo dgy-github/nanocodex \
  --target feat/gui \
  --title "nanocodex GUI v0.1.0" \
  --notes "Windows 桌面版（Tauri v2 + Svelte 5）：CC 四态权限模型、模型快切、可折叠工具输出、彩色 diff、分支/检查点详情、文件预览、Markdown 渲染、当前会话置顶、用量费用估算。安装含开始菜单项与卸载项。"
```
If the tag already exists, just upload/replace the asset instead:
```bash
gh release upload gui-v0.1.0 \
  "D:/agent_prac/ncx-gui/rust/gui/src-tauri/target/release/bundle/nsis/nanocodex_0.1.0_x64-setup.exe" \
  --repo dgy-github/nanocodex --clobber
```

### 4. Verify & report
```bash
gh release view gui-v0.1.0 --repo dgy-github/nanocodex
```
Confirm the asset `nanocodex_0.1.0_x64-setup.exe` is listed, then report the release URL.

## Guardrails
- **Do not** change branches or commit/modify files in the main checkout
  `D:\agent_prac\nanocodex` — concurrent sessions share worktrees and this causes
  branch/file races. This task only reads the installer file and calls `gh --repo`, so
  no checkout switch is needed.
- If `gh` is unauthenticated and no `GH_TOKEN` is available, **stop and ask the user** —
  do not attempt interactive login automatically.
