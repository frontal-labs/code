class FrontalCode < Formula
  desc "High-performance Rust AI agent harness"
  homepage "https://github.com/frontal-labs/code"
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

  head "https://github.com/frontal-labs/code.git", branch: "main"

  depends_on "rust" => :build if build.head?

  def install
    if build.head?
      # cargo installs both commands: `code` and `frontal-code`, which runs `code` beside it.
      system "cargo", "install", *std_cargo_args(path: "cli")
    else
      # The release asset is one raw binary, so both names point at it.
      bin.install Dir["code-cli-*"].first => "code"
      bin.install_symlink "code" => "frontal-code"
    end
  end

  def caveats
    <<~EOS
      To use a provider, set your API key:
        export FRONTAL_API_KEY="<your-api-key>"

      Or use a bearer token with a custom base URL:
        export FRONTAL_AUTH_TOKEN="<your-auth-token>"
        export FRONTAL_BASE_URL="https://api.deepseek.com/anthropic"

      Both `frontal-code` and `code` run the same CLI. Run `frontal-code --help` to see the commands.

      If VS Code's `code` command is already on your PATH, this formula's `code` conflicts with it.
      Keep VS Code's command and use `frontal-code`, or remove the other link first.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/frontal-code --version")
    assert_equal shell_output("#{bin}/code --version"), shell_output("#{bin}/frontal-code --version")
  end
end
