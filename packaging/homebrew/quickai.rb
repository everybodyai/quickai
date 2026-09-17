class Quickai < Formula
  desc "GUI to start/stop local GGUF LLMs (Bonsai, Qwen3) via llama.cpp, Vulkan-accelerated"
  homepage "https://github.com/nujufas/QuickAI"
  version "0.1.0"
  license "PolyForm-Noncommercial-1.0.0"

  on_linux do
    url "https://github.com/nujufas/QuickAI/releases/download/v0.1.0/quickai-0.1.0-linux-x86_64.tar.gz"
    # Placeholder — fill in with the real sha256 after the v0.1.0 GitHub
    # Release is published, e.g. `sha256sum dist/quickai-0.1.0-linux-x86_64.tar.gz`.
    sha256 "0000000000000000000000000000000000000000000000000000000000000"
  end

  # quickai needs llama-server, its .so libs, and models/ sitting right
  # next to its own executable (see the main README), so the whole bundle
  # is installed into libexec as one unit and only a launcher symlink goes
  # into bin — a single `bin.install "quickai"` would separate it from the
  # files it looks for at startup.
  def install
    libexec.install Dir["*"]
    bin.install_symlink libexec/"quickai"
  end

  test do
    assert_predicate libexec/"quickai", :exist?
  end
end
