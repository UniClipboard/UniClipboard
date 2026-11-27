<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="Unic" width="112" height="112" />

# コピーをもっと便利に

<p>
  <strong>Unic（旧称 UniClipboard）</strong> は、Windows・macOS・Linux・iOS・Android・ターミナルに対応したネイティブかつローカルファーストのクリップボード管理＆クロスデバイス同期ツールです。<br />
  履歴検索、クイックパネル、スマートプレビュー、プラグイン拡張、エンドツーエンド暗号化 P2P 同期をひとつに —— アカウント不要、クラウド不要。
</p>

<p>
  <a href="https://uniclipboard.app"><img alt="公式サイト" src="https://img.shields.io/badge/Website-0891B2?style=for-the-badge" /></a>
  <a href="https://docs.uniclipboard.app"><img alt="ドキュメント" src="https://img.shields.io/badge/Docs-4F46E5?style=for-the-badge&logo=readthedocs&logoColor=white" /></a>
  <a href="#community"><img alt="WeChat グループ" src="https://img.shields.io/badge/WeChat-07C160?style=for-the-badge&logo=wechat&logoColor=white" /></a>
  <a href="#community"><img alt="QQ グループ" src="https://img.shields.io/badge/QQ-1688D8?style=for-the-badge&logo=qq&logoColor=white" /></a>
  <a href="https://t.me/uniclipboard"><img alt="Telegram グループ" src="https://img.shields.io/badge/Telegram-26A5E4?style=for-the-badge&logo=telegram&logoColor=white" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/UniClipboard/UniClipboard?style=flat-square&color=E5B650&logo=github&label=Stars" /></a>
  <a href="LICENSE"><img alt="AGPL-3.0 ライセンス" src="https://img.shields.io/badge/License-AGPL--3.0-22c55e?style=flat-square" /></a>
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

- **同期だけではない、本格的なクリップボードワークスペース**：多くのクリップボード管理アプリは単一 OS や有料クラウドに縛られ、逆に同期ツールには使いやすい履歴検索や貼り付け体験がありません。Unic は 1 台の端末だけでも十分に便利——即座に呼び出せるクイックパネル、ローカル履歴検索、自動タグ付け、リッチなリンクプレビューを備え、複数台をつなげばテキスト・画像・ファイル・フォルダごと Windows、macOS、Linux、iOS、Android、ターミナル間で同一 Wi-Fi でも別ネットワークでもシームレスに同期します。
- **Electron の重さとは無縁のネイティブ軽量設計**：Go コアとネイティブデスクトップ UI で構築。バックグラウンドサービス（`unicd`）はアイドル時約 14 MB。Windows のメインウィンドウは約 34–49 MB で、**ウィンドウを閉じると UI プロセスが完全に終了**し、余計なメモリを残しません。
- **口約束ではなく、設計レベルでのプライバシー**：クリップボードにはパスワード、API キー、機密ファイルが入ります。Unic はアカウント登録もクラウド保存も一切不要——フィンガープリント確認のうえ端末同士がエンドツーエンド暗号化で直接つながり、削除したデバイスは即座にアクセスを失い、履歴は自分のデバイスにだけ残ります。
- **コピー＆ペーストを超えて、ワークフローへ拡張**：自動分類、ページタイトルや説明文からも検索できるスマートリンクプレビュー、元の履歴を書き換えない **Action**（書式除去、テキスト変換、画像やファイルの共有リンク生成）、設定画面からワンクリックで入れられる **Go / Lua プラグイン**、さらに SSH・スクリプト・ペアリング不要の 6 桁コード転送に使える本格的な **CLI（`unic`）** まで揃っています。

<a id="features"></a>

## 機能

<table>
  <tr>
    <td>⚡ <strong>クイックパネル</strong></td>
    <td>どのアプリでも <code>⌘ ⌃ V</code>（Windows / Linux は <code>Ctrl Alt V</code>）で履歴を検索し、<code>Enter</code> で直接貼り付け、<code>Shift+Enter</code> でプレーンテキストとして貼り付け。</td>
  </tr>
  <tr>
    <td>🔍 <strong>すぐ見つかるローカル検索</strong></td>
    <td>キーワード入力、または <code>#タグ</code>・<code>@デバイス</code>・<code>/種類</code>・お気に入りで絞り込み。数か月前のコピーや、リンクのページタイトル・説明文からも見つかります。</td>
  </tr>
  <tr>
    <td>🏷️ <strong>自動タグ付け＆スマートプレビュー</strong></td>
    <td>リンク、メール、カラーコード、バージョン番号、UUID、IP アドレスを自動でタグ付け（正規表現の独自ルールも追加可能）。コピーしたリンクはプレビューカードで表示されます。</td>
  </tr>
  <tr>
    <td>🔄 <strong>クロスデバイス同期＆フォルダ転送</strong></td>
    <td>テキスト、リッチテキスト、画像、ファイル、構造を保ったフォルダごと、ペアリング済みの全デバイスまたは特定の 1 台へ同期。</td>
  </tr>
  <tr>
    <td>📮 <strong>ペアリングなしで送信</strong></td>
    <td><code>unic send "テキスト"</code> は転送リンクを表示します。利用できる場合は 6 桁の受け取りコードも表示。どちらでも受け取れ、デバイス記録は残りません。</td>
  </tr>
  <tr>
    <td>🧩 <strong>Action とプラグイン</strong></td>
    <td>元の項目を変えない Action（テキスト変換、書式除去、画像やファイルの共有リンク化）を実行でき、設定から Go / Lua プラグインをワンクリックで追加できます。</td>
  </tr>
  <tr>
    <td>⌨️ <strong>コマンドライン</strong></td>
    <td><code>unic</code> でターミナル、SSH、スクリプトから送受信、Action の実行、履歴一覧の絞り込みができます。</td>
  </tr>
  <tr>
    <td>🌍 <strong>多言語＆テーマ</strong></td>
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

[![Packaging status](https://repology.org/badge/vertical-allrepos/uniclipboard.svg)](https://repology.org/project/uniclipboard/versions)

**2. そのまま使い始める**

普段どおりコピーして、どのアプリからでも `⌘ ⌃ V`（Windows / Linux は `Ctrl Alt V`）でクイックパネルを開き、履歴を検索して貼り付けられます。

**3. 2 台目のデバイスを接続（任意）**

1 台目で **デバイス → デバイスを追加** を開き、6 桁のコードを控えて 2 台目に入力します。両方に同じフィンガープリントが表示されたら、それぞれで **承認** をクリックすると自動同期が始まります。

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

## プライバシー

- デバイス間はエンドツーエンドで暗号化されます。Unic はクリップボードを保存するサーバーを運営していません。
- 新しいデバイスはすべて、フィンガープリントを確認して承認する必要があります。削除したデバイスは即座にアクセスできなくなります。
- クリップボードの内容、ファイルパス、デバイスアドレスはログに記録されません。
- 匿名の診断データにクリップボードの内容やデバイス識別情報は含まれません。**設定 → 診断とプライバシー** でいつでもオフにできます。

セキュリティ上の問題は [SECURITY.md](SECURITY.md) の手順で非公開に報告してください。

## UniClipboard v1 からのアップグレード

v2 は v1 の履歴（テキスト、画像、ファイル、タグ）をすべてインポートできます。v2 を初めて開いたときの案内に従うか、あとで **設定 › ストレージ › v1 から移行** からインポートしてください。

v2 のデバイスは v1 のデバイスと同期できません。すべてのデバイスを v2 にアップグレードし、ペアリングし直してください。v1 のコードは [`v1` ブランチ](https://github.com/UniClipboard/UniClipboard/tree/v1) にあります。

<a id="community"></a>

## コミュニティ

Unic コミュニティに参加して、他のユーザーや開発者と情報交換・要望の共有・不具合報告ができます：

- <img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ グループ：`1080860954`
- <img src="docs/assets/community/wechat.svg" alt="" width="18" /> WeChat：下の QR コードをスキャンしてメンテナーを友だち追加し、グループに参加できます
- <img src="docs/assets/community/telegram.svg" alt="" width="18" /> [Telegram グループ (@uniclipboard)](https://t.me/uniclipboard)

<table>
  <tr>
    <td align="center"><strong><img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ グループ</strong></td>
    <td align="center"><strong><img src="docs/assets/community/wechat.svg" alt="" width="18" /> WeChat グループ</strong></td>
    <td align="center"><strong><img src="docs/assets/community/telegram.svg" alt="" width="18" /> Telegram グループ</strong></td>
  </tr>
  <tr>
    <td align="center"><img src="docs/assets/community/qq-qr.png" alt="Unic QQ グループの QR コード" width="240" /></td>
    <td align="center"><img src="docs/assets/community/wechat-qr.png" alt="Unic WeChat の QR コード" width="240" /></td>
    <td align="center"><a href="https://t.me/uniclipboard"><img src="docs/assets/community/telegram-qr.jpg" alt="Unic Telegram グループの QR コード" width="240" /></a></td>
  </tr>
</table>

## 開発に参加する

```sh
make build            # dist/
go test -race ./...
```

まず [CONTRIBUTING.md](CONTRIBUTING.md) と [アーキテクチャドキュメント](docs/architecture/README.md) をお読みください。プラグインの作り方は [プラグインガイド](plugins/README.md) を参照してください。

## ライセンス

[AGPL-3.0](LICENSE)
