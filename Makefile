.PHONY: all init install install-frontend install-tauri dev build build-macos clean help

# 默认目标：安装所有依赖
all: install

# 初始化子模块
init:
	git submodule sync
	git submodule update --init --recursive

# 安装所有依赖
install: install-frontend install-tauri

# 安装前端依赖
install-frontend:
	cd cinny && npm ci

# 安装 Tauri 依赖
install-tauri:
	npm ci

# 启动开发服务器
dev:
	npm run tauri dev

# 构建生产版本
build:
	npm run tauri build

# 构建 macOS ARM 版本 (Apple Silicon)
build-macos:
	npm run tauri build -- --target aarch64-apple-darwin

# 清理构建产物
clean:
	rm -rf src-tauri/target
	rm -rf cinny/dist

# 显示帮助信息
help:
	@echo "可用命令："
	@echo "  make init             - 初始化/同步子模块"
	@echo "  make install          - 安装所有依赖（前端 + Tauri）"
	@echo "  make install-frontend - 仅安装前端依赖"
	@echo "  make install-tauri    - 仅安装 Tauri 依赖"
	@echo "  make dev              - 启动开发服务器"
	@echo "  make build            - 构建生产版本"
	@echo "  make build-macos      - 构建 macOS ARM 版本 (Apple Silicon)"
	@echo "  make clean            - 清理构建产物"
