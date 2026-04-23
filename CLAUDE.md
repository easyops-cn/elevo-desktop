# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Elevo Desktop 是一个基于 Tauri v2 的 Matrix 即时通讯客户端桌面应用。它将 Elevo Web 客户端（作为 git submodule）封装成原生桌面应用，支持 macOS、Windows 和 Linux 平台。

## 常用命令

```bash
make init          # 初始化/同步子模块
make install       # 安装所有依赖（前端 + Tauri）
make dev           # 启动开发服务器（Tauri 应用）
make dev-web       # 仅启动前端服务器（浏览器访问 http://localhost:8080）
make build         # 构建生产版本
make build-macos   # 构建 macOS universal 版本
make clean         # 清理构建产物
```

## 架构

### 目录结构

- `src-tauri/` - Rust 后端代码（Tauri 应用）
- `cinny/` - Git submodule，包含 Elevo Messenger Web 前端

### Tauri 后端 (src-tauri/)

- [lib.rs](src-tauri/src/lib.rs) - 应用入口，配置 localhost 插件和窗口
- [main.rs](src-tauri/src/main.rs) - 二进制入口点
- [menu.rs](src-tauri/src/menu.rs) - macOS 原生菜单配置（当前被注释）
- [tauri.conf.json](src-tauri/tauri.conf.json) - Tauri 配置（窗口、插件、构建设置）

### 关键技术细节

- 应用现在使用 Tauri custom protocol `tauri://` （开发时仍使用 http）
- 使用 `tauri-plugin-window-state` 保存窗口位置和大小
- 构建时执行 `cd cinny && npm run build`，前端输出到 `cinny/dist`
- 开发时执行 `cd cinny && npm start`，连接 `http://localhost:8080`

### Tauri 插件

- clipboard-manager, notification, fs, shell, http, process, os, dialog
- global-shortcut, updater（仅桌面平台）

### 国际化

- 前端使用 i18next，语言文件位于 `cinny/public/locales/`
- 当前仅支持 en 和 zh 两种语言

## Linux 构建依赖

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

## 版本管理

版本号需在三个位置同步更新：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
