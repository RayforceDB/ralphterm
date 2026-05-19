class Ralphterm < Formula
  desc "Run markdown engineering plans through implementing and reviewing AI agents"
  homepage "https://ralphterm.rayforcedb.com"
  version "0.4.16"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/RayforceDB/ralphterm/releases/download/v0.4.16/ralphterm-aarch64-apple-darwin.tar.xz"
      sha256 "5ac97c4c9b7df3a363c45f8d3c7a5fbdce55a17f521aa1a6525dbf6d45f2a5b2"
    end

    on_intel do
      url "https://github.com/RayforceDB/ralphterm/releases/download/v0.4.16/ralphterm-x86_64-apple-darwin.tar.xz"
      sha256 "bf14233d96f6e90844b0ebbf36fe120649d711e59e1493cda27c1ef12e9518e7"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/RayforceDB/ralphterm/releases/download/v0.4.16/ralphterm-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "f3aa791e67d1f7d71d0881770278c58b47a0884371a8e1a68d32c00d3809dad9"
    end
  end

  def install
    bin.install Dir["*/ralphterm"].first => "ralphterm"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/ralphterm --version")
  end
end
