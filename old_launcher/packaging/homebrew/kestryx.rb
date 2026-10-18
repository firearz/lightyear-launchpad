cask "kestryx" do
  version "0.1.0"
  sha256 :no_check

  url "https://github.com/kestryx/kestryx/releases/download/v#{version}/Kestryx_#{version}_universal.dmg"
  name "Kestryx"
  desc "High-performance open-source Minecraft: Java Edition launcher and CLI"
  homepage "https://github.com/kestryx/kestryx"

  app "Kestryx.app"
  binary "#{appdir}/Kestryx.app/Contents/MacOS/kestryx"

  zap trash: [
    "~/Library/Application Support/Kestryx",
    "~/Library/Caches/dev.kestryx.app",
  ]
end
