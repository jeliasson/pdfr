# Homebrew formula for a personal tap.
#   brew tap OWNER/tap https://github.com/OWNER/homebrew-tap
#   brew install OWNER/tap/pdfr
# Update `url` and `sha256` for each release:
#   curl -L <url> | shasum -a 256
class Pdfr < Formula
  desc "Desktop PDF viewer and form filler with private autocomplete"
  homepage "https://github.com/OWNER/pdfr"
  url "https://github.com/OWNER/pdfr/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_THE_RELEASE_TARBALL"
  license :cannot_represent # PolyForm Strict 1.0.0 (see LICENSE.md)
  head "https://github.com/OWNER/pdfr.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/app")
  end

  test do
    assert_match "pdfr #{version}", shell_output("#{bin}/pdfr --version")
  end
end
