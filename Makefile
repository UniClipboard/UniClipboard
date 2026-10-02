# =========================
# 基础配置
# =========================
CARGO ?= cargo
PKG   := uc-platform

# =========================
# 默认目标
# =========================
.PHONY: help
help:
	@echo "Available targets:"
	@echo "  make build        # build uc-platform library"
	@echo "  make check        # cargo check for uc-platform"
	@echo "  make clean        # cargo clean"

# =========================
# 构建
# =========================
.PHONY: build
build:
	$(CARGO) build -p $(PKG)

# =========================
# 快速检查（不产物）
# =========================
.PHONY: check
check:
	$(CARGO) check -p $(PKG)

# =========================
# 清理
# =========================
.PHONY: clean
clean:
	$(CARGO) clean

