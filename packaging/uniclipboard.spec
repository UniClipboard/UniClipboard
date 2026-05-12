# UniClipboard COPR spec — binary repackage 模式。
#
# 注:RPM 会把注释里凡是 % 开头的 token 当 macro 展开(Fedora 上仅 warning,
# 但 EPEL 上直接 abort 整个 spec 解析,Name/Version 字段全部丢失),所以
# 注释里提及 macro 名字时要先 % 转义为 %% — 见下方 %%install / %%{_arch}。
#
# 该 spec 不从源码编译，而是把 GitHub Release 上 Tauri 直接产出的 binary RPM
# 当作 Source0,在 %%install 阶段用 rpm2cpio 解出来重新打包。这样 COPR mock
# chroot 不需要 Rust/Node/webkit2gtk-devel 等重型 BuildRequires,构建时间从
# 30 min 压到 1 min 以内,并保留 upstream binary 一致性(同一个二进制在
# Releases 页和 dnf copr 渠道里发出去)。
#
# 版本号规范:
#   - %%{version}      RPM-合法版本,prerelease 后缀用 ~ 替换 -,例如 0.7.0~alpha.7
#                      RPM 比较语义中 ~ 比任意字符小,因此 0.7.0~alpha.7 < 0.7.0
#   - %%{upstream_tag} 上游 git tag/文件名里的原始版本,保留 -,例如 0.7.0-alpha.7
#
# 版本注入路径:
#   1. CI(.github/workflows/copr.yml)在 `cp spec` 之后用 sed 把
#      @VERSION@ / @UPSTREAM_TAG@ 占位符替换为实际值,SRPM 里 spec 因此
#      已固化版本号 — COPR mock chroot 二次 build 不再依赖任何 macro。
#   2. 本地手动构建仍可用 `rpmbuild -bs --define "_version 0.7.0~alpha.7"
#      --define "_upstream_tag 0.7.0-alpha.7" ...`,macro 优先于占位符。
#
# 历史教训:之前 fallback 写死成具体版本号(0.7.0-alpha.7),CI 用
# `--define` 注入只在 GH runner 那次 rpmbuild 生效,SRPM 进入 COPR
# chroot 二次 build 后 _upstream_tag 不存在 → 走 fallback → 期望旧版本
# 文件名 → "Bad file: UniClipboard-0.7.0-alpha.7-...rpm" 失败。

%global upstream_tag %{?_upstream_tag}%{!?_upstream_tag:@UPSTREAM_TAG@}
%global debug_package %{nil}

Name:           uniclipboard
Version:        %{?_version}%{!?_version:@VERSION@}
Release:        1%{?dist}
Summary:        Privacy-first end-to-end encrypted cross-device clipboard sync

License:        Apache-2.0 OR MIT
URL:            https://github.com/UniClipboard/UniClipboard

# Tauri 在 release.yml 中输出的 binary RPM 命名 = UniClipboard-<tag>-1.<arch>.rpm
# 同时声明两个 arch 的 Source — SRPM 里把两个 binary RPM 都打包进来,
# COPR mock chroot 在不同 arch 二次 build 时用 %%ifarch 选对应 Source。
# 不能用 %%{_arch} 嵌入文件名搭配 `rpmbuild -bs --target` 多次出 SRPM:
# SRPM 文件名只由 N-V-R 决定、不带 arch 后缀,多次 -bs 会同名覆盖。
Source0:        https://github.com/UniClipboard/UniClipboard/releases/download/v%{upstream_tag}/UniClipboard-%{upstream_tag}-1.x86_64.rpm
Source1:        https://github.com/UniClipboard/UniClipboard/releases/download/v%{upstream_tag}/UniClipboard-%{upstream_tag}-1.aarch64.rpm

ExclusiveArch:  x86_64 aarch64

# 运行时依赖 — 与 Tauri v2 + webkit2gtk-4.1 栈一致。
# 包名在 Fedora 与 RHEL/openSUSE 系略有差异,这里用 Fedora 主线名;COPR
# 默认 chroot 都是 Fedora/EPEL,后续要扩展到 openSUSE 再加 conditional。
Requires:       webkit2gtk4.1
Requires:       gtk3
Requires:       libappindicator-gtk3
Requires:       librsvg2

%description
UniClipboard is a privacy-first, end-to-end encrypted, cross-device clipboard
sync tool built with Rust and Tauri.

This package repackages the upstream binary RPM published on GitHub Releases.
The binary is byte-identical to what users would download from the Releases
page; this package only re-signs and re-indexes it for the dnf/COPR channel.

%prep
# Source0/Source1 是完整 binary RPM,按 host arch 选对应那一个,用 rpm2cpio
# 解到当前工作目录 (BUILD/<name>-<ver>/)。
mkdir -p uniclipboard-%{version}
cd uniclipboard-%{version}
%ifarch x86_64
rpm2cpio %{SOURCE0} | cpio -idm
%endif
%ifarch aarch64
rpm2cpio %{SOURCE1} | cpio -idm
%endif

%build
# 无 — 二进制已 upstream 编译完毕

%install
cd uniclipboard-%{version}
mkdir -p %{buildroot}
# 上游 RPM payload 是绝对路径 (/usr/bin/..., /usr/share/...),解出来后是
# 当前目录下 ./usr/...,直接 cp -a 到 buildroot 即可。
cp -a usr %{buildroot}/

# 动态收集所有文件 — 避免 size 变更或资源新增时手动维护 %%files 列表。
find %{buildroot} \( -type f -o -type l \) -printf '/%%P\n' | sort > %{_builddir}/uniclipboard.filelist

%files -f %{_builddir}/uniclipboard.filelist

%changelog
# changelog 由 CI 在 build 时通过 `--define` 注入或追加;此处保留占位。
* Sat May 09 2026 mkdir700 <release@uniclipboard.app> - 0.7.0~alpha.7-1
- Initial COPR packaging — binary repackage of upstream GitHub release
