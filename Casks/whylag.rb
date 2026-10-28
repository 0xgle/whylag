cask "whylag" do
  arch arm: "aarch64", intel: "x64"

  version "0.2.0"
  sha256 arm:   "795518c2b5bcbbdcf4875e5d6cb546405c196c72d652ce193b95bb3f37c8a370",
         intel: "0a4b98ebaf6110b1efc6d154cb5aba4a86f9625db0cc89a8c1b51ef9f857a549"

  url "https://github.com/0xgle/whylag/releases/download/v#{version}/WhyLag_#{version}_#{arch}.dmg"
  name "WhyLag"
  desc "Network diagnostics, Wi-Fi survey and troubleshooting"
  homepage "https://0xgle.github.io/whylag/"

  app "WhyLag.app"

  zap trash: [
    "~/Library/Application Support/eu.m8.whylag",
    "~/Library/Caches/eu.m8.whylag",
  ]
end
