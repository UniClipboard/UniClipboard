<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="Unic" width="112" height="112" />

# 一度コピーすれば、どのデバイスでも貼り付け

<p>
  <strong>Unic（旧称 UniClipboard）</strong> はテキスト・画像・ファイル・フォルダをデバイス間で直接同期します。<br />
  エンドツーエンド暗号化。アカウント不要、クラウド不要。
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="ダウンロード" src="https://img.shields.io/badge/%E3%83%80%E3%82%A6%E3%83%B3%E3%83%AD%E3%83%BC%E3%83%89-2563eb?style=for-the-badge&logo=github&logoColor=white" /></a>
  <a href="https://testflight.apple.com/join/nyNQ8dQe"><img alt="iOS Beta" src="https://img.shields.io/badge/iOS%20Beta-1f2937?style=for-the-badge&logo=apple&logoColor=white" /></a>
  <a href="#quick-start"><img alt="はじめる" src="https://img.shields.io/badge/%E3%81%AF%E3%81%98%E3%82%81%E3%82%8B-4b5563?style=for-the-badge" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Windows" src="https://img.shields.io/badge/Windows-1f2937?style=flat-square&logo=data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAxNiAxNiIgZmlsbD0iIzM4YmRmOCI+PHBhdGggZD0iTTAgMGg3LjR2Ny40SDB6bTguNiAwSDE2djcuNEg4LjZ6TTAgOC42aDcuNFYxNkgwem04LjYgMEgxNlYxNkg4LjZ6Ii8+PC9zdmc+" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="macOS" src="https://img.shields.io/badge/macOS-1f2937?style=flat-square&logo=apple&logoColor=white" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Linux" src="https://img.shields.io/badge/Linux-1f2937?style=flat-square&logo=linux&logoColor=facc15" /></a>
  <a href="https://testflight.apple.com/join/nyNQ8dQe"><img alt="iOS Beta" src="https://img.shields.io/badge/iOS-Beta-2563eb?style=flat-square&logo=apple&logoColor=white&labelColor=1f2937" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Android" src="https://img.shields.io/badge/Android-1f2937?style=flat-square&logo=android&logoColor=3ddc84" /></a>
  <a href="#cli"><img alt="CLI" src="https://img.shields.io/badge/CLI-1f2937?style=flat-square&logo=gnubash&logoColor=white" /></a>
</p>

<p>
  <small>
    <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | <strong>日本語</strong> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>
  </small>
</p>

![デスクトップ・タブレット・スマートフォンでの Unic](docs/assets/product-overview.png)

</div>

---

## Unic を選ぶ理由

- **クロスプラットフォーム**：Windows、macOS、Linux、iOS、Android の間で同期。同じ Wi-Fi でも別のネットワークでもつながります。
- **クラウドを経由しない**：デバイス同士が暗号化された接続で直接つながります。登録不要で、履歴は自分のデバイスにだけ保存されます。
- **軽量**：バックグラウンド同期サービスはアイドル時に約 14 MB<sup>[1](#benchmarks)</sup>。Windows ではメインウィンドウが約 34–49 MB<sup>[2](#benchmarks)</sup>。閉じるとそのプロセスは終了します。
- **何でも同期**：テキスト、画像、ファイル、フォルダごと。

<a id="features"></a>

## 機能

<table>
  <tr>
    <td>⚡ <strong>クイックパネル</strong></td>
    <td>どのアプリでも <code>⌘ ⌃ V</code>（Windows / Linux は <code>Ctrl Alt V</code>）で履歴を開き、選んで Enter で貼り付け。</td>
  </tr>
  <tr>
    <td>🔍 <strong>すぐ見つかる</strong></td>
    <td>キーワード入力、または <code>#タグ</code>・<code>@デバイス</code>・<code>/種類</code> で絞り込み。数か月前のコピーも見つかります。</td>
  </tr>
  <tr>
    <td>🏷️ <strong>自動タグ付け</strong></td>
    <td>リンク、メール、カラーコード、バージョン番号、UUID、IP アドレスを自動でタグ付け。独自ルールも追加できます。</td>
  </tr>
  <tr>
    <td>📁 <strong>ファイルとフォルダ</strong></td>
    <td>フォルダを構造ごと送信。全デバイスにも、特定のデバイスにも送れます。</td>
  </tr>
  <tr>
    <td>📮 <strong>ペアリングなしで送信</strong></td>
    <td><code>unic send "テキスト"</code> は転送リンクを表示します。利用できる場合は、6 桁の受け取りコードも表示します。受け取りにはどちらも使えます。デバイスの記録は残りません。</td>
  </tr>
  <tr>
    <td>🧩 <strong>プラグイン</strong></td>
    <td>設定からワンクリックでインストール。例：画像やファイルを共有リンクに変換。</td>
  </tr>
  <tr>
    <td>⌨️ <strong>コマンドライン</strong></td>
    <td><code>unic</code> でターミナル、SSH、スクリプトから送受信や履歴一覧の絞り込みができます。</td>
  </tr>
  <tr>
    <td>🌍 <strong>多言語</strong></td>
    <td>English、简体中文、日本語、Português、Русский。ライト／ダークテーマ対応。</td>
  </tr>
</table>

<a id="quick-start"></a>

## 3 ステップで開始

**1. ダウンロードしてインストール**

- **デスクトップ**：[GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) から Windows（`.exe`/`.zip`）、macOS（`.dmg`）、Linux（`.deb`/`.rpm`/`.AppImage`）向けインストーラーをダウンロードします。
- **iOS**：[TestFlight](https://testflight.apple.com/join/nyNQ8dQe) からパブリックベータに参加できます。
- **Android**：[GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) から APK をダウンロードしてインストールします（現在 Google Play での公開予定はありません）。
- **CLI のみ**：`npm install -g @uniclipboard/cli` を実行します。

**2. 2 台目のデバイスを接続**

1 台目で **デバイス → デバイスを追加** を開き、6 桁のコードを控えて 2 台目に入力します。両方に同じフィンガープリントが表示されたら、それぞれで **承認** をクリックします。

**3. コピーする**

一方のデバイスで `Ctrl+C`、もう一方で `Ctrl+V`。

<a id="cli"></a>

## コマンドライン

```sh
unic send --to all "Hello"        # 全デバイスへ送信
unic send --to all -f ./photos    # ファイルやフォルダを送信
unic send "一時的な内容"            # ペアリング不要：転送リンクと、利用可能な場合は受け取りコードを表示
unic recv 482913                  # コードで受信
unic history list                 # 履歴を表示
```

すべてのコマンドは [CLI ガイド](app/service/README.md) を参照してください。

<a id="benchmarks"></a>

## 実測値

| | メモリ | 条件 |
|---|---:|---|
| 同期サービス `unicd` | **14 MB** | macOS（Apple Silicon）、履歴なし、20 秒アイドル後に計測。アクティビティモニタの「メモリ」列と同じ指標<sup>1</sup> |
| メインウィンドウ | **34 MB** / 49 MB | Windows 11、ネイティブ ARM64 / x64 ビルド、テキスト 60 件。タスクマネージャーの指標<sup>2</sup> |
| メインウィンドウ（画像あり） | 119 MB / 133 MB | 同上に 2048×1536 の画像 12 枚を追加<sup>2</sup> |

<sup>1</sup> コミット `e2cb68d` で `footprint <pid>` により計測。<sup>2</sup> 3 回の中央値。詳細は [Windows メモリレポート](docs/exec/windows-desktop-memory.md)。

## プライバシー

- デバイス間はエンドツーエンドで暗号化されます。Unic はクリップボードを保存するサーバーを運営していません。
- 新しいデバイスはすべて、フィンガープリントを確認して承認する必要があります。削除したデバイスは即座にアクセスできなくなります。
- クリップボードの内容、ファイルパス、デバイスアドレスはログに記録されません。
- 匿名の診断データにクリップボードの内容やデバイス識別情報は含まれません。**設定 → 診断とプライバシー** でいつでもオフにできます。

セキュリティ上の問題は [SECURITY.md](SECURITY.md) の手順で非公開に報告してください。

## UniClipboard v1 からのアップグレード

v2 は v1 の履歴（テキスト、画像、ファイル、タグ）をすべてインポートできます。v2 を初めて開いたときの案内に従うか、あとで **設定 › ストレージ › v1 から移行** からインポートしてください。

v2 のデバイスは v1 のデバイスと同期できません。すべてのデバイスを v2 にアップグレードし、ペアリングし直してください。v1 のコードは [`v1` ブランチ](https://github.com/UniClipboard/UniClipboard/tree/v1) にあります。

## 開発に参加する

```sh
make build            # dist/
go test -race ./...
```

まず [CONTRIBUTING.md](CONTRIBUTING.md) と [アーキテクチャドキュメント](docs/architecture/README.md) をお読みください。プラグインの作り方は [プラグインガイド](plugins/README.md) を参照してください。

## ライセンス

[AGPL-3.0](LICENSE)
