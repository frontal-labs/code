class Frontal Code < Formula
  desc "High-performance Rust AI agent harness"
  homepage "https://github.com/frontal-labs/frontal-code"
  license "MIT"
  version "0.1.1"

  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/frontal-labs/frontal-code/releases/download/v#{version}/frontal-code-macos-arm64.tar.gz"
      sha256 "e2513b12a1000b6c6fb5c96df030e9d1218a218032da54117c9ebee20c076688"
    else
      url "https://github.com/frontal-labs/frontal-code/releases/download/v#{version}/frontal-code-macos-x64.tar.gz"
      sha256 "<INSERT_X64_SHA256>" # filled by release CI (macos-13 runner)
    end
  end

  head "https://github.com/frontal-labs/frontal-code.git", branch: "main"

  depends_on "rust" => :build if build.head?

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "crates/cli")
    else
      bin.install "bin/frontal-code"
    end
  end

  def caveats
    <<~EOS
      To use a provider, set your API key:
        export FRONTAL_API_KEY="sk-ant-..."

      Or use a bearer token with a custom base URL:
        export FRONTAL_AUTH_TOKEN="sk-..."
        export FRONTAL_BASE_URL="https://api.deepseek.com/anthropic"

      Run `frontal-code --help` to see available commands.
    EOS
  end

  test do
    assert_match "frontal-code", shell_output("#{bin}/frontal-code --version")
  end
end
