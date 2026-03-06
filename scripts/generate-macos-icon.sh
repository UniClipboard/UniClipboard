#!/bin/bash

# macOS 应用图标生成脚本
# 用途: 从源图标生成带圆角的 .icns 文件
# 
# 使用方法:
# ./scripts/generate-macos-icon.sh <source-icon.png> [--keep-background]
#
# 参数:
# --keep-background: 保留原始背景,但添加圆角和透明边缘
#
# 要求:
# - 源图标必须是 1024x1024 或更大的 PNG 文件
# - 需要安装 ImageMagick: brew install imagemagick

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
ICONSET_DIR="src-tauri/icons/icon.iconset"
OUTPUT_ICNS="src-tauri/icons/icon.icns"

if [ ! -f "$SOURCE_ICON" ]; then
    echo "错误: 找不到源图标文件: $SOURCE_ICON"
    exit 1
fi

# 检查是否安装了 ImageMagick
if ! command -v convert &> /dev/null; then
    echo "错误: 需要安装 ImageMagick"
    echo "请运行: brew install imagemagick"
    exit 1
fi

if [ "$KEEP_BACKGROUND" = true ]; then
    echo "模式: 保留背景 + 添加圆角"
else
    echo "检查源图标是否有透明背景..."
    if ! sips -g hasAlpha "$SOURCE_ICON" | grep -q "hasAlpha: yes"; then
        echo "警告: 源图标没有透明背景(alpha 通道)"
        echo ""
        echo "如果你想保留背景但添加圆角,请使用:"
        echo "  ./scripts/generate-macos-icon.sh $SOURCE_ICON --keep-background"
        echo ""
        echo "如果你想移除背景,请使用图像编辑工具处理后再运行此脚本"
        exit 1
    fi
fi

echo "创建 iconset 目录..."
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

# 定义圆角处理函数
# macOS 图标圆角半径约为 22.37% (基于苹果设计规范)
apply_rounded_corners() {
    local input="$1"
    local output="$2"
    local size="$3"
    
    # 计算圆角半径 (约 22.37% 的图标尺寸)
    local radius=$(echo "$size * 0.2237" | bc | awk '{print int($1+0.5)}')
    
    if [ "$KEEP_BACKGROUND" = true ]; then
        # 保留背景,添加圆角蒙版
        convert "$input" \
            \( +clone -alpha extract \
               -draw "fill black polygon 0,0 0,$radius $radius,0 fill white circle $radius,$radius $radius,0" \
               \( +clone -flip \) -compose Multiply -composite \
               \( +clone -flop \) -compose Multiply -composite \
            \) -alpha off -compose CopyOpacity -composite \
            "$output"
    else
        # 已有透明背景,直接复制
        cp "$input" "$output"
    fi
}

echo "生成各种尺寸的图标..."

# 生成临时文件
TEMP_16="$ICONSET_DIR/temp_16.png"
TEMP_32="$ICONSET_DIR/temp_32.png"
TEMP_64="$ICONSET_DIR/temp_64.png"
TEMP_128="$ICONSET_DIR/temp_128.png"
TEMP_256="$ICONSET_DIR/temp_256.png"
TEMP_512="$ICONSET_DIR/temp_512.png"
TEMP_1024="$ICONSET_DIR/temp_1024.png"

sips -z 16 16     "$SOURCE_ICON" --out "$TEMP_16" > /dev/null 2>&1
sips -z 32 32     "$SOURCE_ICON" --out "$TEMP_32" > /dev/null 2>&1
sips -z 64 64     "$SOURCE_ICON" --out "$TEMP_64" > /dev/null 2>&1
sips -z 128 128   "$SOURCE_ICON" --out "$TEMP_128" > /dev/null 2>&1
sips -z 256 256   "$SOURCE_ICON" --out "$TEMP_256" > /dev/null 2>&1
sips -z 512 512   "$SOURCE_ICON" --out "$TEMP_512" > /dev/null 2>&1
sips -z 1024 1024 "$SOURCE_ICON" --out "$TEMP_1024" > /dev/null 2>&1

# 应用圆角
apply_rounded_corners "$TEMP_16" "$ICONSET_DIR/icon_16x16.png" 16
apply_rounded_corners "$TEMP_32" "$ICONSET_DIR/icon_16x16@2x.png" 32
apply_rounded_corners "$TEMP_32" "$ICONSET_DIR/icon_32x32.png" 32
apply_rounded_corners "$TEMP_64" "$ICONSET_DIR/icon_32x32@2x.png" 64
apply_rounded_corners "$TEMP_128" "$ICONSET_DIR/icon_128x128.png" 128
apply_rounded_corners "$TEMP_256" "$ICONSET_DIR/icon_128x128@2x.png" 256
apply_rounded_corners "$TEMP_256" "$ICONSET_DIR/icon_256x256.png" 256
apply_rounded_corners "$TEMP_512" "$ICONSET_DIR/icon_256x256@2x.png" 512
apply_rounded_corners "$TEMP_512" "$ICONSET_DIR/icon_512x512.png" 512
apply_rounded_corners "$TEMP_1024" "$ICONSET_DIR/icon_512x512@2x.png" 1024

# 清理临时文件
rm -f "$TEMP_16" "$TEMP_32" "$TEMP_64" "$TEMP_128" "$TEMP_256" "$TEMP_512" "$TEMP_1024"

echo "生成 .icns 文件..."
iconutil -c icns "$ICONSET_DIR" -o "$OUTPUT_ICNS"

echo "清理临时文件..."
rm -rf "$ICONSET_DIR"

echo "✅ 成功生成 macOS 图标: $OUTPUT_ICNS"
if [ "$KEEP_BACKGROUND" = true ]; then
    echo "   (已保留背景并添加圆角)"
fi
echo ""
echo "下一步:"
echo "1. 重新构建应用: cd src-tauri && cargo build --release"
echo "2. 或直接打包: bun run tauri build"
echo ""
echo "验证图标:"
echo "  sips -g hasAlpha $OUTPUT_ICNS"
