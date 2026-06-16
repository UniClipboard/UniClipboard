cask "uniclipboard" do
  arch arm: "aarch64", intel: "x64"

  version "__VERSION__"
  sha256 arm: "__SHA256_ARM__",
         intel: "__SHA256_INTEL__"

  url "https://github.com/UniClipboard/UniClipboard/releases/download/v#{version}/UniClipboard_#{version}_#{arch}.dmg",
      verified: "github.com/UniClipboard/UniClipboard/"
  name "UniClipboard"
  desc "Privacy-first cross-device clipboard sync"
  homepage "https://www.uniclipboard.app/"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :monterey"

  app "UniClipboard.app"

  zap trash: [
    "~/Library/Application Support/app.uniclipboard.desktop",
    "~/Library/Caches/app.uniclipboard.desktop",
    "~/Library/Logs/app.uniclipboard.desktop",
  ]
end
