# Flathub 打包

本目录是提交到 [Flathub](https://github.com/flathub) 的 Flatpak 包源：

- `app.uniclipboard.desktop.yml` — Flatpak manifest（app-id = Tauri identifier）
- `app.uniclipboard.desktop.metainfo.xml` — AppStream 元数据（Flathub 强制要求）

## 重要：Flathub 不进 Repology

和 winget 一样，**Repology 不抓 Flathub**，这一步不会改变 packaging-status 徽章。做它的理由：Flathub 是 Linux 桌面应用覆盖面最大的分发渠道（远超 deb/rpm/AppImage 总和），补齐最大的真实分发缺口。
```bash
flatpak install flathub app.uniclipboard.desktop
```

## 这是几个渠道里最难的一个

不像 nixpkgs/Scoop 能较快落地，Flathub 首次提交通常需要明显的现场调试。已知卡点：

1. **WebKitGTK 版本匹配**：deb 二进制链接系统 `webkit2gtk-4.1`（GTK3）。需确认所选 `org.gnome.Platform` 版本提供同一 ABI；若不匹配，要换 runtime 版本或把 webkitgtk 作为 extension/bundle。`runtime-version: '46'` 是起点，按构建报错调整。
2. **剪贴板权限**：Tauri 在 Wayland 沙箱里读系统剪贴板依赖 `wlr-data-control` 协议，flatpak 默认可能受限。装好后务必实测复制/粘贴是否真的跨设备同步，必要时调 `finish-args` 或走 portal。
3. **AppStream 截图**：`metainfo.xml` 里截图 URL 是占位，**必须** 换成真实可访问的图片（提交一张到仓库 `assets/` 再引用），否则 Flathub CI 的 `appstream-util validate` 不过。
4. **release date**：`metainfo.xml` 里 0.15.0 的 `date` 按实际发布日核对。

## 提交流程

1. **本地构建验证**（必做，需 `flatpak-builder`）
   ```bash
   flatpak install flathub org.gnome.Platform//46 org.gnome.Sdk//46
   # 先填 manifest 里两个 deb 的 sha256（全 0 占位）：
   #   sha256sum 下载好的 .deb，或与 release SHA256SUMS.txt 核对（勿用沙箱产出的 hash）
   flatpak-builder --user --install --force-clean build-dir packaging/flathub/app.uniclipboard.desktop.yml
   flatpak run app.uniclipboard.desktop      # 起、托盘、配对、复制粘贴同步全测一遍
   ```
   校验 metadata：
   ```bash
   flatpak run org.freedesktop.appstream-glib validate \
     packaging/flathub/app.uniclipboard.desktop.metainfo.xml
   ```

2. **申请上架**：fork [`flathub/flathub`](https://github.com/flathub/flathub)，基于 `new-pr` 分支提交 manifest + metainfo，开 PR。Flathub reviewer 会过权限、构建、AppStream。合并后会为本应用建独立仓库 `flathub/app.uniclipboard.desktop`，后续版本在那里维护（可启用 Flatpak External Data Checker 自动跟随 release）。

## 待确认

- manifest/metainfo 经事实校对（app-id、deb 名、命令名、权限依据 `snap/snapcraft.yaml` 的 plugs 推导），但 **构建与运行未在本环境验证**，需你按上面流程实测调通。
