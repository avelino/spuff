#!/bin/bash
# Custom tools installation script
#
# This is an example of tools you might install beyond
# what spuff provides by default.
#
# To use: Add to your dotfiles repo as install.sh
# or reference in cloud-init runcmd.

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

log() {
    echo -e "${GREEN}[INSTALL]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Kubernetes tools
install_k8s_tools() {
    log "Installing Kubernetes tools..."

    # kubectl
    curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl"
    chmod +x kubectl
    sudo mv kubectl /usr/local/bin/

    # k9s
    curl -sL https://github.com/derailed/k9s/releases/latest/download/k9s_Linux_amd64.tar.gz | tar xz
    sudo mv k9s /usr/local/bin/

    # helm
    curl https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash

    log "Kubernetes tools installed"
}

# AWS CLI
install_aws_cli() {
    log "Installing AWS CLI..."

    curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o "awscliv2.zip"
    unzip -q awscliv2.zip
    sudo ./aws/install
    rm -rf aws awscliv2.zip

    log "AWS CLI installed"
}

# Terraform
install_terraform() {
    log "Installing Terraform..."

    TERRAFORM_VERSION="1.6.0"
    curl -LO "https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_linux_amd64.zip"
    unzip -q "terraform_${TERRAFORM_VERSION}_linux_amd64.zip"
    sudo mv terraform /usr/local/bin/
    rm "terraform_${TERRAFORM_VERSION}_linux_amd64.zip"

    log "Terraform installed"
}

# GitHub CLI
install_gh() {
    log "Installing GitHub CLI..."

    type -p curl >/dev/null || sudo apt install curl -y
    curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
    sudo chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null
    sudo apt update
    sudo apt install gh -y

    log "GitHub CLI installed"
}

# lazygit
install_lazygit() {
    log "Installing lazygit..."

    LAZYGIT_VERSION=$(curl -s "https://api.github.com/repos/jesseduffield/lazygit/releases/latest" | grep -Po '"tag_name": "v\K[^"]*')
    curl -Lo lazygit.tar.gz "https://github.com/jesseduffield/lazygit/releases/latest/download/lazygit_${LAZYGIT_VERSION}_Linux_x86_64.tar.gz"
    tar xf lazygit.tar.gz lazygit
    sudo install lazygit /usr/local/bin
    rm lazygit lazygit.tar.gz

    log "lazygit installed"
}

# Main
main() {
    log "Starting custom tools installation..."

    # Uncomment the tools you want:
    # install_k8s_tools
    # install_aws_cli
    # install_terraform
    install_gh
    install_lazygit

    log "Custom tools installation complete!"
}

main "$@"
