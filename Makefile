# =========================
# 基础配置
# =========================
CARGO ?= cargo
PKG   := uc-platform
EXAMPLE := clipboard_probe

# =========================
# 默认目标
# =========================
.PHONY: help
help:
	@echo "Available targets:"
	@echo "  make build        # build uc-platform library"
	@echo "  make run          # run uc-platform example (clipboard_probe)"
	@echo "  make check        # cargo check for uc-platform"
	@echo "  make clean        # cargo clean"

# =========================
# 构建
# =========================
.PHONY: build
build:
	$(CARGO) build -p $(PKG)

# =========================
# 运行 example
# =========================
.PHONY: run
run:
	$(CARGO) run -p $(PKG) --example $(EXAMPLE)

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

