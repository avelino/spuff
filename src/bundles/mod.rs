//! Language bundles for spuff project configuration.
//!
//! Each bundle provides a pre-configured set of tools for a specific language/stack:
//! - Compiler/runtime
//! - Language server (LSP)
//! - Formatter and linter
//! - Debug tools
//!
//! Bundles are installed by the spuff-agent on the remote VM.

use serde::{Deserialize, Serialize};

/// A single tool within a bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleTool {
    /// Tool identifier
    pub id: &'static str,
    /// Display name
    pub name: &'static str,
    /// Installation command (shell)
    pub install_cmd: &'static str,
    /// Command to get version after install
    pub version_cmd: &'static str,
    /// Whether this tool is required (vs optional)
    pub required: bool,
}

/// A language bundle definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// Bundle identifier (e.g., "rust", "go")
    pub id: &'static str,
    /// Display name
    pub name: &'static str,
    /// Description
    pub description: &'static str,
    /// Tools included in this bundle
    pub tools: Vec<BundleTool>,
}

impl Bundle {
    /// Get installation script for this bundle
    pub fn install_script(&self, username: &str) -> String {
        let mut script = format!(
            "#!/bin/bash\nset -e\n# Installing {} bundle\n\n",
            self.name
        );

        for tool in &self.tools {
            script.push_str(&format!(
                "echo 'Installing {}...'\n{}\n\n",
                tool.name, tool.install_cmd
            ));
        }

        // Replace {{username}} placeholder
        script.replace("{{username}}", username)
    }
}

/// Get bundle definition by ID
pub fn get_bundle(id: &str) -> Option<Bundle> {
    match id {
        "rust" => Some(rust_bundle()),
        "go" => Some(go_bundle()),
        "python" => Some(python_bundle()),
        "node" => Some(node_bundle()),
        "elixir" => Some(elixir_bundle()),
        "java" => Some(java_bundle()),
        "zig" => Some(zig_bundle()),
        "cpp" => Some(cpp_bundle()),
        "ruby" => Some(ruby_bundle()),
        _ => None,
    }
}

/// Get all available bundles
pub fn all_bundles() -> Vec<Bundle> {
    vec![
        rust_bundle(),
        go_bundle(),
        python_bundle(),
        node_bundle(),
        elixir_bundle(),
        java_bundle(),
        zig_bundle(),
        cpp_bundle(),
        ruby_bundle(),
    ]
}

/// Get list of valid bundle IDs
pub fn valid_bundle_ids() -> Vec<&'static str> {
    vec!["rust", "go", "python", "node", "elixir", "java", "zig", "cpp", "ruby"]
}

// =============================================================================
// Bundle Definitions
// =============================================================================

fn rust_bundle() -> Bundle {
    Bundle {
        id: "rust",
        name: "Rust",
        description: "Rust toolchain with cargo, rust-analyzer, clippy, rustfmt",
        tools: vec![
            BundleTool {
                id: "rustup",
                name: "rustup + cargo",
                install_cmd: r#"
if ! command -v rustup &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
source "$HOME/.cargo/env" 2>/dev/null || true
# Ensure components are installed
rustup component add clippy rustfmt 2>/dev/null || true
"#,
                version_cmd: "rustc --version",
                required: true,
            },
            BundleTool {
                id: "rust-analyzer",
                name: "rust-analyzer",
                install_cmd: r#"
source "$HOME/.cargo/env" 2>/dev/null || true
rustup component add rust-analyzer 2>/dev/null || true
"#,
                version_cmd: "rust-analyzer --version 2>/dev/null || echo 'installed via rustup'",
                required: true,
            },
            BundleTool {
                id: "mold",
                name: "mold (fast linker)",
                install_cmd: r#"
ARCH=$(uname -m)
MOLD_VERSION="2.34.1"
if [ "$ARCH" = "x86_64" ]; then
    curl -fsSL "https://github.com/rui314/mold/releases/download/v${MOLD_VERSION}/mold-${MOLD_VERSION}-x86_64-linux.tar.gz" | tar -xz -C /tmp
    sudo cp /tmp/mold-${MOLD_VERSION}-x86_64-linux/bin/mold /usr/local/bin/
    sudo chmod +x /usr/local/bin/mold
elif [ "$ARCH" = "aarch64" ]; then
    curl -fsSL "https://github.com/rui314/mold/releases/download/v${MOLD_VERSION}/mold-${MOLD_VERSION}-aarch64-linux.tar.gz" | tar -xz -C /tmp
    sudo cp /tmp/mold-${MOLD_VERSION}-aarch64-linux/bin/mold /usr/local/bin/
    sudo chmod +x /usr/local/bin/mold
fi
"#,
                version_cmd: "mold --version",
                required: false,
            },
            BundleTool {
                id: "cargo-watch",
                name: "cargo-watch",
                install_cmd: r#"
source "$HOME/.cargo/env" 2>/dev/null || true
cargo install cargo-watch 2>/dev/null || true
"#,
                version_cmd: "cargo watch --version 2>/dev/null || echo 'not installed'",
                required: false,
            },
        ],
    }
}

fn go_bundle() -> Bundle {
    Bundle {
        id: "go",
        name: "Go",
        description: "Go toolchain with gopls, delve, golangci-lint",
        tools: vec![
            BundleTool {
                id: "go",
                name: "Go",
                install_cmd: r#"
GO_VERSION="1.23.4"
ARCH=$(uname -m)
case $ARCH in
    x86_64) GO_ARCH="amd64" ;;
    aarch64) GO_ARCH="arm64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac
curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-${GO_ARCH}.tar.gz" -o /tmp/go.tar.gz
sudo rm -rf /usr/local/go
sudo tar -C /usr/local -xzf /tmp/go.tar.gz
rm /tmp/go.tar.gz
# Add to PATH
echo 'export PATH=$PATH:/usr/local/go/bin:$HOME/go/bin' >> $HOME/.bashrc
export PATH=$PATH:/usr/local/go/bin:$HOME/go/bin
"#,
                version_cmd: "/usr/local/go/bin/go version",
                required: true,
            },
            BundleTool {
                id: "gopls",
                name: "gopls (LSP)",
                install_cmd: r#"
export PATH=$PATH:/usr/local/go/bin:$HOME/go/bin
go install golang.org/x/tools/gopls@latest
"#,
                version_cmd: "$HOME/go/bin/gopls version 2>/dev/null || echo 'installed'",
                required: true,
            },
            BundleTool {
                id: "delve",
                name: "delve (debugger)",
                install_cmd: r#"
export PATH=$PATH:/usr/local/go/bin:$HOME/go/bin
go install github.com/go-delve/delve/cmd/dlv@latest
"#,
                version_cmd: "$HOME/go/bin/dlv version 2>/dev/null || echo 'installed'",
                required: true,
            },
            BundleTool {
                id: "golangci-lint",
                name: "golangci-lint",
                install_cmd: r#"
curl -sSfL https://raw.githubusercontent.com/golangci/golangci-lint/master/install.sh | sh -s -- -b $HOME/go/bin v1.62.2
"#,
                version_cmd: "$HOME/go/bin/golangci-lint --version 2>/dev/null || echo 'installed'",
                required: false,
            },
            BundleTool {
                id: "air",
                name: "air (live reload)",
                install_cmd: r#"
export PATH=$PATH:/usr/local/go/bin:$HOME/go/bin
go install github.com/air-verse/air@latest
"#,
                version_cmd: "$HOME/go/bin/air -v 2>/dev/null || echo 'installed'",
                required: false,
            },
        ],
    }
}

fn python_bundle() -> Bundle {
    Bundle {
        id: "python",
        name: "Python",
        description: "Python 3.12+ with uv, ruff, pyright",
        tools: vec![
            BundleTool {
                id: "python",
                name: "Python 3.12",
                install_cmd: r#"
sudo add-apt-repository -y ppa:deadsnakes/ppa 2>/dev/null || true
sudo apt-get update
sudo apt-get install -y python3.12 python3.12-venv python3.12-dev python3-pip
sudo update-alternatives --install /usr/bin/python3 python3 /usr/bin/python3.12 1 2>/dev/null || true
"#,
                version_cmd: "python3 --version",
                required: true,
            },
            BundleTool {
                id: "uv",
                name: "uv (package manager)",
                install_cmd: r#"
curl -LsSf https://astral.sh/uv/install.sh | sh
echo 'eval "$(~/.local/bin/uv generate-shell-completion bash)"' >> $HOME/.bashrc 2>/dev/null || true
"#,
                version_cmd: "$HOME/.local/bin/uv --version 2>/dev/null || uv --version",
                required: true,
            },
            BundleTool {
                id: "ruff",
                name: "ruff (linter/formatter)",
                install_cmd: r#"
$HOME/.local/bin/uv tool install ruff 2>/dev/null || pip install ruff
"#,
                version_cmd: "ruff --version 2>/dev/null || echo 'installed'",
                required: true,
            },
            BundleTool {
                id: "pyright",
                name: "pyright (LSP)",
                install_cmd: r#"
# Pyright requires Node.js, install via npm if available
if command -v npm &> /dev/null; then
    npm install -g pyright
else
    pip install pyright
fi
"#,
                version_cmd: "pyright --version 2>/dev/null || echo 'installed'",
                required: true,
            },
            BundleTool {
                id: "ipython",
                name: "IPython",
                install_cmd: r#"
pip install ipython
"#,
                version_cmd: "ipython --version 2>/dev/null || echo 'installed'",
                required: false,
            },
        ],
    }
}

fn node_bundle() -> Bundle {
    Bundle {
        id: "node",
        name: "Node.js",
        description: "Node.js 22 LTS with npm, pnpm, TypeScript",
        tools: vec![
            BundleTool {
                id: "nodejs",
                name: "Node.js 22 LTS",
                install_cmd: r#"
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt-get install -y nodejs
"#,
                version_cmd: "node --version",
                required: true,
            },
            BundleTool {
                id: "pnpm",
                name: "pnpm",
                install_cmd: r#"
npm install -g pnpm
"#,
                version_cmd: "pnpm --version",
                required: true,
            },
            BundleTool {
                id: "typescript",
                name: "TypeScript",
                install_cmd: r#"
npm install -g typescript typescript-language-server
"#,
                version_cmd: "tsc --version",
                required: true,
            },
            BundleTool {
                id: "eslint",
                name: "ESLint",
                install_cmd: r#"
npm install -g eslint
"#,
                version_cmd: "eslint --version",
                required: false,
            },
            BundleTool {
                id: "prettier",
                name: "Prettier",
                install_cmd: r#"
npm install -g prettier
"#,
                version_cmd: "prettier --version",
                required: false,
            },
        ],
    }
}

fn elixir_bundle() -> Bundle {
    Bundle {
        id: "elixir",
        name: "Elixir",
        description: "Erlang/OTP + Elixir with mix, elixir-ls",
        tools: vec![
            BundleTool {
                id: "erlang",
                name: "Erlang/OTP",
                install_cmd: r#"
# Install Erlang from Erlang Solutions
wget https://packages.erlang-solutions.com/erlang-solutions_2.0_all.deb
sudo dpkg -i erlang-solutions_2.0_all.deb
rm erlang-solutions_2.0_all.deb
sudo apt-get update
sudo apt-get install -y esl-erlang
"#,
                version_cmd: "erl -version 2>&1 | head -1",
                required: true,
            },
            BundleTool {
                id: "elixir",
                name: "Elixir",
                install_cmd: r#"
sudo apt-get install -y elixir
mix local.hex --force
mix local.rebar --force
"#,
                version_cmd: "elixir --version | head -2 | tail -1",
                required: true,
            },
            BundleTool {
                id: "elixir-ls",
                name: "elixir-ls (LSP)",
                install_cmd: r#"
mkdir -p $HOME/.local/share/elixir-ls
cd $HOME/.local/share/elixir-ls
ELIXIR_LS_VERSION="0.24.1"
curl -fsSL "https://github.com/elixir-lsp/elixir-ls/releases/download/v${ELIXIR_LS_VERSION}/elixir-ls-v${ELIXIR_LS_VERSION}.zip" -o elixir-ls.zip
unzip -o elixir-ls.zip
chmod +x language_server.sh
rm elixir-ls.zip
"#,
                version_cmd: "echo 'elixir-ls installed'",
                required: true,
            },
            BundleTool {
                id: "phoenix",
                name: "Phoenix Framework",
                install_cmd: r#"
mix archive.install hex phx_new --force
"#,
                version_cmd: "mix phx.new --version 2>/dev/null || echo 'installed'",
                required: false,
            },
        ],
    }
}

fn java_bundle() -> Bundle {
    Bundle {
        id: "java",
        name: "Java",
        description: "OpenJDK 21 with Maven, Gradle, jdtls",
        tools: vec![
            BundleTool {
                id: "openjdk",
                name: "OpenJDK 21",
                install_cmd: r#"
sudo apt-get install -y openjdk-21-jdk
echo 'export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64' >> $HOME/.bashrc
"#,
                version_cmd: "java --version | head -1",
                required: true,
            },
            BundleTool {
                id: "maven",
                name: "Maven",
                install_cmd: r#"
sudo apt-get install -y maven
"#,
                version_cmd: "mvn --version | head -1",
                required: true,
            },
            BundleTool {
                id: "gradle",
                name: "Gradle",
                install_cmd: r#"
GRADLE_VERSION="8.12"
curl -fsSL "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" -o /tmp/gradle.zip
sudo unzip -d /opt/gradle /tmp/gradle.zip
sudo ln -sf /opt/gradle/gradle-${GRADLE_VERSION}/bin/gradle /usr/local/bin/gradle
rm /tmp/gradle.zip
"#,
                version_cmd: "gradle --version | grep Gradle",
                required: false,
            },
            BundleTool {
                id: "jdtls",
                name: "jdtls (LSP)",
                install_cmd: r#"
mkdir -p $HOME/.local/share/jdtls
cd $HOME/.local/share/jdtls
JDTLS_VERSION="1.40.0"
curl -fsSL "https://download.eclipse.org/jdtls/milestones/${JDTLS_VERSION}/jdt-language-server-${JDTLS_VERSION}-202409261450.tar.gz" | tar xz
"#,
                version_cmd: "echo 'jdtls installed'",
                required: true,
            },
        ],
    }
}

fn zig_bundle() -> Bundle {
    Bundle {
        id: "zig",
        name: "Zig",
        description: "Zig compiler with zls",
        tools: vec![
            BundleTool {
                id: "zig",
                name: "Zig",
                install_cmd: r#"
ZIG_VERSION="0.13.0"
ARCH=$(uname -m)
case $ARCH in
    x86_64) ZIG_ARCH="x86_64" ;;
    aarch64) ZIG_ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac
curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}.tar.xz" | tar -xJ -C /tmp
sudo mv /tmp/zig-linux-${ZIG_ARCH}-${ZIG_VERSION} /usr/local/zig
sudo ln -sf /usr/local/zig/zig /usr/local/bin/zig
"#,
                version_cmd: "zig version",
                required: true,
            },
            BundleTool {
                id: "zls",
                name: "zls (LSP)",
                install_cmd: r#"
ZLS_VERSION="0.13.0"
ARCH=$(uname -m)
case $ARCH in
    x86_64) ZLS_ARCH="x86_64" ;;
    aarch64) ZLS_ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac
curl -fsSL "https://github.com/zigtools/zls/releases/download/${ZLS_VERSION}/zls-${ZLS_ARCH}-linux.tar.xz" | tar -xJ -C /tmp
sudo mv /tmp/zls /usr/local/bin/
sudo chmod +x /usr/local/bin/zls
"#,
                version_cmd: "zls --version",
                required: true,
            },
        ],
    }
}

fn cpp_bundle() -> Bundle {
    Bundle {
        id: "cpp",
        name: "C/C++",
        description: "GCC, Clang, CMake, ninja, clangd",
        tools: vec![
            BundleTool {
                id: "gcc",
                name: "GCC",
                install_cmd: r#"
sudo apt-get install -y gcc g++ build-essential
"#,
                version_cmd: "gcc --version | head -1",
                required: true,
            },
            BundleTool {
                id: "clang",
                name: "Clang/LLVM",
                install_cmd: r#"
sudo apt-get install -y clang clang-format clang-tidy lldb
"#,
                version_cmd: "clang --version | head -1",
                required: true,
            },
            BundleTool {
                id: "cmake",
                name: "CMake",
                install_cmd: r#"
sudo apt-get install -y cmake
"#,
                version_cmd: "cmake --version | head -1",
                required: true,
            },
            BundleTool {
                id: "ninja",
                name: "Ninja",
                install_cmd: r#"
sudo apt-get install -y ninja-build
"#,
                version_cmd: "ninja --version",
                required: true,
            },
            BundleTool {
                id: "clangd",
                name: "clangd (LSP)",
                install_cmd: r#"
sudo apt-get install -y clangd
"#,
                version_cmd: "clangd --version | head -1",
                required: true,
            },
            BundleTool {
                id: "gdb",
                name: "GDB",
                install_cmd: r#"
sudo apt-get install -y gdb
"#,
                version_cmd: "gdb --version | head -1",
                required: false,
            },
        ],
    }
}

fn ruby_bundle() -> Bundle {
    Bundle {
        id: "ruby",
        name: "Ruby",
        description: "Ruby with bundler, solargraph, rubocop",
        tools: vec![
            BundleTool {
                id: "ruby",
                name: "Ruby",
                install_cmd: r#"
sudo apt-get install -y ruby-full ruby-dev
"#,
                version_cmd: "ruby --version",
                required: true,
            },
            BundleTool {
                id: "bundler",
                name: "Bundler",
                install_cmd: r#"
sudo gem install bundler
"#,
                version_cmd: "bundler --version",
                required: true,
            },
            BundleTool {
                id: "solargraph",
                name: "Solargraph (LSP)",
                install_cmd: r#"
sudo gem install solargraph
"#,
                version_cmd: "solargraph --version",
                required: true,
            },
            BundleTool {
                id: "rubocop",
                name: "RuboCop",
                install_cmd: r#"
sudo gem install rubocop
"#,
                version_cmd: "rubocop --version",
                required: false,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_bundle_rust() {
        let bundle = get_bundle("rust").unwrap();
        assert_eq!(bundle.id, "rust");
        assert!(!bundle.tools.is_empty());
    }

    #[test]
    fn test_get_bundle_invalid() {
        assert!(get_bundle("invalid").is_none());
    }

    #[test]
    fn test_all_bundles_count() {
        let bundles = all_bundles();
        assert_eq!(bundles.len(), 9);
    }

    #[test]
    fn test_valid_bundle_ids() {
        let ids = valid_bundle_ids();
        assert!(ids.contains(&"rust"));
        assert!(ids.contains(&"go"));
        assert!(ids.contains(&"python"));
        assert!(!ids.contains(&"invalid"));
    }

    #[test]
    fn test_install_script_generation() {
        let bundle = get_bundle("rust").unwrap();
        let script = bundle.install_script("dev");
        assert!(script.contains("Installing Rust"));
        assert!(script.contains("rustup"));
    }

    #[test]
    fn test_bundle_has_required_tools() {
        for bundle in all_bundles() {
            let required_count = bundle.tools.iter().filter(|t| t.required).count();
            assert!(required_count > 0, "Bundle {} has no required tools", bundle.id);
        }
    }
}
