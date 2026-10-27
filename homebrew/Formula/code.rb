class FrontalCode < Formula
  desc "High-performance Rust AI agent harness"
  homepage "https://github.com/frontal-labs/frontal-code"
  license "Apache-2.0"
  version "0.1.1"

  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/frontal-labs/code/releases/download/v#{version}/code-cli-aarch64-apple-darwin"
      sha256 "<INSERT_ARM64_SHA256>" # filled by release CI (macos-14 runner)
    else
      url "https://github.com/frontal-labs/code/releases/download/v#{version}/code-cli-x86_64-apple-darwin"
      sha256 "<INSERT_X64_SHA256>" # filled by release CI (macos-13 runner)
    end
  end

  head "https://github.com/frontal-labs/frontal-code.git", branch: "main"

  depends_on "rust" => :build if build.head?

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "cli")
      # cargo installs the `code` bin; expose it under the package name like the release build.
      mv bin/"code", bin/"frontal-code"
    else
      # Release asset is a raw binary named code-cli-<target>; install it as frontal-code.
      bin.install Dir["code-cli-*"].first => "frontal-code"
    end
  end

  def caveats
    <<~EOS
      To use a provider, set your API key:
        export FRONTAL_API_KEY="<your-api-key>"

      Or use a bearer token with a custom base URL:
        export FRONTAL_AUTH_TOKEN="<your-auth-token>"
        export FRONTAL_BASE_URL="https://api.deepseek.com/anthropic"

      Run `frontal-code --help` to see available commands.
    EOS
  end

  test do
    assert_match "frontal-code", shell_output("#{bin}/frontal-code --version")
  end
end
