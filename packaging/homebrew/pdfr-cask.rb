# Homebrew cask for macOS: the app bundle plus the `pdfr` command.
#   brew install --cask kjellberg/tap/pdfr
# Copied to the tap as Casks/pdfr.rb once macOS releases are switched on
# (RELEASE_MACOS); the release workflow fills in version and sha256.
cask "pdfr" do
  version "0.1.0"
  sha256 "REPLACE_WITH_SHA256_OF_THE_ZIP"

  url "https://github.com/kjellberg/pdfr/releases/download/v#{version}/pdfr-v#{version}-macos.zip"
  name "pdfr"
  desc "Fill in PDF forms the way they were meant to be filled in"
  homepage "https://github.com/kjellberg/pdfr"

  depends_on macos: ">= :monterey"

  app "pdfr.app"
  binary "#{appdir}/pdfr.app/Contents/MacOS/pdfr"

  zap trash: "~/Library/Application Support/pdfr"
end
