# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Elevo Desktop 是一个基于 Tauri v2 的 Matrix 即时通讯客户端桌面应用。它将 Elevo Web 客户端（作为 git submodule）封装成原生桌面应用，支持 macOS、Windows 和 Linux 平台。

## 常用命令

```bash
make init          # 初始化/同步子模块
make install       # 安装所有依赖（前端 + Tauri）
make dev           # 启动开发服务器
make build         # 构建生产版本
make build-macos   # 构建 macOS universal 版本
make clean         # 清理构建产物
```

### 发布

```bash
# 生成 release.json（CI 自动调用）
GITHUB_TOKEN=xxx npm run release
```

## 架构

### 目录结构

- `src-tauri/` - Rust 后端代码（Tauri 应用）
- `cinny/` - Git submodule，包含 Cinny Web 前端
- `scripts/release.mjs` - 发布自动化脚本

### Tauri 后端 (src-tauri/)

- [lib.rs](src-tauri/src/lib.rs) - 应用入口，配置 localhost 插件和窗口
- [main.rs](src-tauri/src/main.rs) - 二进制入口点
- [menu.rs](src-tauri/src/menu.rs) - macOS 原生菜单配置（当前被注释）
- [tauri.conf.json](src-tauri/tauri.conf.json) - Tauri 配置（窗口、插件、构建设置）

### 关键技术细节

- 应用通过 `tauri-plugin-localhost` 在端口 44548 提供前端服务
- 使用 `tauri-plugin-window-state` 保存窗口位置和大小
- 构建时执行 `cd cinny && npm run build`，前端输出到 `cinny/dist`
- 开发时执行 `cd cinny && npm start`，连接 `http://localhost:8080`

### Tauri 插件

- clipboard-manager, notification, fs, shell, http, process, os, dialog
- global-shortcut, updater（仅桌面平台）

## Linux 构建依赖

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

## 版本管理

版本号需在三个位置同步更新：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
