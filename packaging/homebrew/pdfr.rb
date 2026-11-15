# Homebrew formula for a personal tap (Linux and macOS, built from source).
# macOS users get the app bundle from the cask instead: pdfr-cask.rb.
#   brew tap kjellberg/tap https://github.com/kjellberg/homebrew-tap
#   brew install kjellberg/tap/pdfr
# The release workflow updates `url` and `sha256` in the tap.
class Pdfr < Formula
  desc "Fill in PDF forms the way they were meant to be filled in"
  homepage "https://github.com/kjellberg/pdfr"
  url "https://github.com/kjellberg/pdfr/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_THE_RELEASE_TARBALL"
  license :cannot_represent # PolyForm Strict 1.0.0 (see LICENSE.md)
  head "https://github.com/kjellberg/pdfr.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/app")
  end

  test do
    assert_match "pdfr #{version}", shell_output("#{bin}/pdfr --version")
  end
end
