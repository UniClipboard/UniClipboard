#!/bin/bash

# 跨平台应用图标生成脚本
# 用途: 从源图标生成 macOS/Windows/Linux 所需的所有图标文件
# 
# 使用方法:
# ./scripts/generate-all-icons.sh <source-icon.png> [--keep-background]
#
# 参数:
# --keep-background: 保留原始背景,但为需要圆角的平台添加圆角
#
# 要求:
# - 源图标必须是 1024x1024 或更大的 PNG 文件
# - macOS: 需要安装 ImageMagick (brew install imagemagick)
# - Windows: 需要安装 ImageMagick
# - Linux: 需要安装 ImageMagick

set -e

SOURCE_ICON=""
KEEP_BACKGROUND=false

# 解析参数
for arg in "$@"; do
    case $arg in
        --keep-background)
            KEEP_BACKGROUND=true
            shift
            ;;
        *)
            if [ -z "$SOURCE_ICON" ]; then
                SOURCE_ICON="$arg"
            fi
            ;;
    esac
done

SOURCE_ICON="${SOURCE_ICON:-assets/logo.png}"
ICONS_DIR="src-tauri/icons"

if [ ! -f "$SOURCE_ICON" ]; then
    echo "错误: 找不到源图标文件: $SOURCE_ICON"
    exit 1
fi

# 检查是否安装了 ImageMagick
if ! command -v convert &> /dev/null; then
    echo "错误: 需要安装 ImageMagick"
    echo "macOS: brew install imagemagick"
    echo "Linux: sudo apt install imagemagick (Debian/Ubuntu) 或 sudo dnf install imagemagick (Fedora)"
    exit 1
fi

if [ "$KEEP_BACKGROUND" = true ]; then
    echo "🎨 模式: 保留背景 + 添加圆角(适用于 macOS/Linux)"
else
    echo "🎨 模式: 透明背景"
    if ! sips -g hasAlpha "$SOURCE_ICON" 2>/dev/null | grep -q "hasAlpha: yes"; then
        echo "警告: 源图标没有透明背景"
        echo ""
        echo "如果你想保留背景但添加圆角,请使用:"
        echo "  ./scripts/generate-all-icons.sh $SOURCE_ICON --keep-background"
        echo ""
        read -p "是否继续? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
fi

# 圆角处理函数 (用于 macOS 和 Linux)
apply_rounded_corners() {
    local input="$1"
    local output="$2"
    local size="$3"
    
    # macOS/Linux 圆角半径约为 22.37%
    local radius=$(echo "$size * 0.2237" | bc | awk '{print int($1+0.5)}')
    
    if [ "$KEEP_BACKGROUND" = true ]; then
        convert "$input" \
            \( +clone -alpha extract \
               -draw "fill black polygon 0,0 0,$radius $radius,0 fill white circle $radius,$radius $radius,0" \
               \( +clone -flip \) -compose Multiply -composite \
               \( +clone -flop \) -compose Multiply -composite \
            \) -alpha off -compose CopyOpacity -composite \
            "$output"
    else
        cp "$input" "$output"
    fi
}

echo ""
echo "📦 生成 PNG 图标 (Linux 使用)..."
mkdir -p "$ICONS_DIR"

# Linux 需要的 PNG 尺寸
sips -z 32 32 "$SOURCE_ICON" --out "$ICONS_DIR/temp_32.png" > /dev/null 2>&1
sips -z 128 128 "$SOURCE_ICON" --out "$ICONS_DIR/temp_128.png" > /dev/null 2>&1
sips -z 256 256 "$SOURCE_ICON" --out "$ICONS_DIR/temp_256.png" > /dev/null 2>&1

apply_rounded_corners "$ICONS_DIR/temp_32.png" "$ICONS_DIR/32x32.png" 32
apply_rounded_corners "$ICONS_DIR/temp_128.png" "$ICONS_DIR/128x128.png" 128
apply_rounded_corners "$ICONS_DIR/temp_256.png" "$ICONS_DIR/128x128@2x.png" 256

rm -f "$ICONS_DIR/temp_32.png" "$ICONS_DIR/temp_128.png" "$ICONS_DIR/temp_256.png"

echo "✅ PNG 图标生成完成"

echo ""
echo "🍎 生成 macOS .icns 图标..."
./scripts/generate-macos-icon.sh "$SOURCE_ICON" $([ "$KEEP_BACKGROUND" = true ] && echo "--keep-background")

echo ""
echo "🪟 生成 Windows .ico 图标..."

# Windows .ico 文件包含多个尺寸,不需要圆角
# 生成临时 PNG 文件
TEMP_PNGS=""
for size in 16 32 48 64 128 256; do
    temp_file="$ICONS_DIR/temp_${size}.png"
    sips -z $size $size "$SOURCE_ICON" --out "$temp_file" > /dev/null 2>&1
    TEMP_PNGS="$TEMP_PNGS $temp_file"
done

# 使用 ImageMagick 合并为 .ico
convert $TEMP_PNGS "$ICONS_DIR/icon.ico"

# 清理临时文件
rm -f $TEMP_PNGS

echo "✅ Windows .ico 图标生成完成"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✨ 所有平台图标生成完成!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "生成的文件:"
echo "  macOS:   $ICONS_DIR/icon.icns"
echo "  Windows: $ICONS_DIR/icon.ico"
echo "  Linux:   $ICONS_DIR/32x32.png"
echo "           $ICONS_DIR/128x128.png"
echo "           $ICONS_DIR/128x128@2x.png"
echo ""
echo "下一步:"
echo "  bun run tauri build"
echo ""
