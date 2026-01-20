use tera::{Context, Tera};

use crate::config::AppConfig;
use crate::error::Result;
use crate::project_config::ProjectConfig;

const CLOUD_INIT_TEMPLATE: &str = r#"#cloud-config
package_update: true
package_upgrade: false

# Disable root SSH login
disable_root: true
ssh_pwauth: false

# Minimal packages - devtools installed via agent
packages:
  - git
  - curl
  - unzip
  - zip
  - build-essential
  - mosh
  - jq
  - htop

users:
  - name: {{ username }}
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    groups: [sudo]
    lock_passwd: true
    ssh_authorized_keys:
      - {{ ssh_public_key }}
{% if spuff_public_key %}      - {{ spuff_public_key }}
{% endif %}

write_files:
  # Store username for agent devtools installation
  - path: /opt/spuff/username
    permissions: '0644'
    content: "{{ username }}"

  # Devtools configuration for agent
  - path: /opt/spuff/devtools.json
    permissions: '0644'
    content: |
      {
        "docker": true,
        "shell_tools": true,
        "nodejs": true,
        "claude_code": true,
        "environment": {% if environment %}"{{ environment }}"{% else %}null{% endif %},
        "dotfiles": {% if dotfiles %}"{{ dotfiles }}"{% else %}null{% endif %},
        "tailscale": {% if tailscale_enabled %}true{% else %}false{% endif %},
        "tailscale_authkey": {% if tailscale_authkey %}"{{ tailscale_authkey }}"{% else %}null{% endif %}
      }

{% if has_project_config %}
  # Project configuration from spuff.yaml
  - path: /opt/spuff/project.json
    permissions: '0644'
    content: |
      {{ project_config }}
{% endif %}

  # Bootstrap script - minimal setup + agent start
  - path: /opt/spuff/bootstrap.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      set -e

      STATUS_FILE="/opt/spuff/bootstrap.status"
      LOG_FILE="/var/log/spuff-bootstrap.log"
      USERNAME="{{ username }}"

      log() {
        echo "[$(date '+%H:%M:%S')] $1" | tee -a "$LOG_FILE"
      }

      update_status() {
        echo "$1" > "$STATUS_FILE"
        log "Status: $1"
      }

      # Initialize
      update_status "starting"
      exec > >(tee -a "$LOG_FILE") 2>&1

      log "Starting minimal bootstrap for user: $USERNAME"

      ###################
      # Install spuff-agent
      ###################
      update_status "installing:agent"

      # Install spuff-agent (skip if already uploaded via --dev mode)
      if [ ! -x /opt/spuff/spuff-agent ]; then
        log "Installing spuff-agent..."
        ARCH=$(uname -m)
        case $ARCH in
          x86_64) ARCH="x86_64" ;;
          aarch64) ARCH="aarch64" ;;
          *) log "Unsupported architecture: $ARCH" ;;
        esac
        # Download to temp first to avoid truncating existing file
        if curl -fsSL "https://github.com/avelino/spuff/releases/latest/download/spuff-agent-linux-${ARCH}" -o /tmp/spuff-agent; then
          mv /tmp/spuff-agent /opt/spuff/spuff-agent
          chmod +x /opt/spuff/spuff-agent
          log "spuff-agent downloaded from GitHub"
        else
          log "spuff-agent download failed (release may not exist yet)"
        fi
      else
        log "spuff-agent already present (dev mode upload)"
      fi

      # Start spuff-agent
      systemctl daemon-reload
      systemctl enable spuff-agent || true
      systemctl start spuff-agent || true
      log "spuff-agent started"

      # Agent is ready - devtools will be installed via agent API
      update_status "ready"
      log "Bootstrap completed! Devtools will be installed via agent."

  # Bootstrap service
  - path: /etc/systemd/system/spuff-bootstrap.service
    permissions: '0644'
    content: |
      [Unit]
      Description=Spuff Bootstrap - Minimal environment setup
      After=network-online.target cloud-final.service
      Wants=network-online.target

      [Service]
      Type=oneshot
      ExecStart=/opt/spuff/bootstrap.sh
      RemainAfterExit=yes
      StandardOutput=journal
      StandardError=journal

      [Install]
      WantedBy=multi-user.target

  - path: /opt/spuff/idle-checker.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      IDLE_TIMEOUT_SECONDS={{ idle_timeout_seconds }}
      LAST_ACTIVITY_FILE="/tmp/spuff-last-activity"

      update_activity() {
        date +%s > "$LAST_ACTIVITY_FILE"
      }

      check_idle() {
        if [ ! -f "$LAST_ACTIVITY_FILE" ]; then
          update_activity
          return
        fi

        LAST_ACTIVITY=$(cat "$LAST_ACTIVITY_FILE")
        NOW=$(date +%s)
        IDLE_TIME=$((NOW - LAST_ACTIVITY))

        # Check for active SSH sessions
        if who | grep -q pts; then
          update_activity
          return
        fi

        # Check for running processes (claude, node, cargo, etc)
        if pgrep -f "claude|node|cargo|python|go build" > /dev/null; then
          update_activity
          return
        fi

        if [ "$IDLE_TIME" -ge "$IDLE_TIMEOUT_SECONDS" ]; then
          logger "spuff: Idle timeout reached ($IDLE_TIME seconds). Shutting down."
          shutdown -h now
        fi
      }

      check_idle

  - path: /etc/cron.d/spuff-idle-checker
    content: |
      */5 * * * * root /opt/spuff/idle-checker.sh >> /var/log/spuff-idle.log 2>&1

  - path: /etc/systemd/system/spuff-agent.service
    permissions: '0644'
    content: |
      [Unit]
      Description=Spuff Agent - Remote dev environment monitor
      After=network.target spuff-bootstrap.service

      [Service]
      Type=simple
      ExecStart=/opt/spuff/spuff-agent
      Restart=always
      RestartSec=5
      Environment=RUST_LOG=info
      EnvironmentFile=-/opt/spuff/agent.env

      [Install]
      WantedBy=multi-user.target

{% if agent_token %}
  - path: /opt/spuff/agent.env
    permissions: '0600'
    content: |
      SPUFF_AGENT_TOKEN={{ agent_token }}
{% endif %}

  # Shell configuration with modern tools
  - path: {{ home_dir }}/.bashrc.d/spuff.sh
    permissions: '0644'
    content: |
      # Spuff shell enhancements

      # Show bootstrap status on login
      _spuff_status() {
        local status_file="/opt/spuff/bootstrap.status"
        if [ -f "$status_file" ]; then
          local status=$(cat "$status_file")
          if [ "$status" != "ready" ]; then
            echo -e "\033[33m[spuff] Bootstrap in progress: $status\033[0m"
            echo -e "\033[33m        Watch: tail -f /var/log/spuff-bootstrap.log\033[0m"
            echo ""
          fi
        fi
      }
      _spuff_status

      # Colors
      export CLICOLOR=1
      export TERM=xterm-256color

      # Better history
      export HISTSIZE=10000
      export HISTFILESIZE=20000
      export HISTCONTROL=ignoreboth:erasedups
      shopt -s histappend

      # Aliases for modern tools (if installed)
      if command -v eza &> /dev/null; then
        alias ls='eza --icons --group-directories-first'
        alias ll='eza -la --icons --group-directories-first'
        alias la='eza -a --icons --group-directories-first'
        alias lt='eza --tree --icons --level=2'
      else
        alias ll='ls -la --color=auto'
        alias la='ls -A --color=auto'
      fi

      if command -v bat &> /dev/null; then
        alias cat='bat --paging=never'
        alias less='bat'
      fi

      if command -v rg &> /dev/null; then
        alias grep='rg'
      fi

      if command -v fd &> /dev/null; then
        alias find='fd'
      elif command -v fdfind &> /dev/null; then
        alias fd='fdfind'
        alias find='fdfind'
      fi

      # fzf configuration
      if command -v fzf &> /dev/null; then
        export FZF_DEFAULT_OPTS='--height 40% --layout=reverse --border --color=fg:#f8f8f2,bg:#282a36,hl:#bd93f9 --color=fg+:#f8f8f2,bg+:#44475a,hl+:#bd93f9 --color=info:#ffb86c,prompt:#50fa7b,pointer:#ff79c6 --color=marker:#ff79c6,spinner:#ffb86c,header:#6272a4'
        if command -v fd &> /dev/null; then
          export FZF_DEFAULT_COMMAND='fd --type f --hidden --follow --exclude .git'
        elif command -v fdfind &> /dev/null; then
          export FZF_DEFAULT_COMMAND='fdfind --type f --hidden --follow --exclude .git'
        fi
        export FZF_CTRL_T_COMMAND="$FZF_DEFAULT_COMMAND"

        # Enable fzf keybindings
        [ -f /usr/share/doc/fzf/examples/key-bindings.bash ] && source /usr/share/doc/fzf/examples/key-bindings.bash
        [ -f ~/.fzf.bash ] && source ~/.fzf.bash
      fi

      # zoxide (smart cd)
      if command -v zoxide &> /dev/null; then
        eval "$(zoxide init bash)"
      fi

      # Starship prompt
      if command -v starship &> /dev/null; then
        eval "$(starship init bash)"
      fi

      # Useful functions
      mkcd() { mkdir -p "$1" && cd "$1"; }

      # Check bootstrap status
      spuff-status() {
        if [ -f /opt/spuff/bootstrap.status ]; then
          echo "Bootstrap status: $(cat /opt/spuff/bootstrap.status)"
        fi
        echo ""
        echo "Installed tools:"
        command -v docker &>/dev/null && echo "  [x] docker" || echo "  [ ] docker"
        command -v fzf &>/dev/null && echo "  [x] fzf" || echo "  [ ] fzf"
        command -v bat &>/dev/null && echo "  [x] bat" || echo "  [ ] bat"
        command -v eza &>/dev/null && echo "  [x] eza" || echo "  [ ] eza"
        command -v zoxide &>/dev/null && echo "  [x] zoxide" || echo "  [ ] zoxide"
        command -v starship &>/dev/null && echo "  [x] starship" || echo "  [ ] starship"
        command -v node &>/dev/null && echo "  [x] node $(node -v 2>/dev/null)" || echo "  [ ] node"
        command -v claude &>/dev/null && echo "  [x] claude" || echo "  [ ] claude"
      }

      # Git shortcuts
      alias g='git'
      alias gs='git status'
      alias ga='git add'
      alias gc='git commit'
      alias gp='git push'
      alias gl='git log --oneline --graph'
      alias gd='git diff'

      # Docker shortcuts
      alias d='docker'
      alias dc='docker compose'
      alias dps='docker ps'

      # Spuff info (only show when bootstrap is ready)
      if [ -f /opt/spuff/bootstrap.status ] && [ "$(cat /opt/spuff/bootstrap.status)" = "ready" ]; then
        echo -e "\033[36m+---------------------------+\033[0m"
        echo -e "\033[36m|  s p u f f                |\033[0m"
        echo -e "\033[36m|  ephemeral dev env        |\033[0m"
        echo -e "\033[36m+---------------------------+\033[0m"
        echo ""
      fi

  # Starship minimal config
  - path: {{ home_dir }}/.config/starship.toml
    permissions: '0644'
    content: |
      # Minimal starship config for spuff
      format = """
      $directory$git_branch$git_status$character"""

      [character]
      success_symbol = "[>](bold green)"
      error_symbol = "[>](bold red)"

      [directory]
      style = "bold cyan"
      truncation_length = 3
      truncate_to_repo = true

      [git_branch]
      symbol = "git:"
      style = "bold purple"

      [git_status]
      style = "bold yellow"

  # Pre-authorized SSH host keys for common git servers
  - path: /etc/ssh/ssh_known_hosts
    permissions: '0644'
    content: |
      # GitHub
      github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl
      github.com ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=
      github.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6+PKCUXaDbC7qtbW8gIkhL7aGCsOr/C56SJMy/BCZfxd1nWzAOxSDPgVsmerOBYfNqltV9/hWCqBywINIR+5dIg6JTJ72pcEpEjcYgXkE2YEFXV1JHnsKgbLWNlhScqb2UmyRkQyytRLtL+38TGxkxCflmO+5Z8CSSNY7GidjMIZ7Q4zMjA2n1nGrlTDkzwDCsw+wqFPGQA179cnfGWOWRVruj16z6XyvxvjJwbz0wQZ75XK5tKSb7FNyeIEs4TT4jk+S4dhPeAUC5y+bDYirYgM4GC7uEnztnZyaVWQ7B381AK4Qdrwt51ZqExKbQpTUNn+EjqoTwvqNj4kqx5QUCI0ThS/YkOxJCXmPUWZbhjpCg56i+2aB6CmK2JGhn57K5mj0MNdBXA4/WnwH6XoPWJzK5Nyu2zB3nAZp+S5hpQs+p1vN1/wsjk=
      # GitLab
      gitlab.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAfuCHKVTjquxvt6CM6tdG4SLp1Btn/nOeHHE5UOzRdf
      gitlab.com ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFSMqzJeV9rUzU4kWitGjeR4PWSa29SPqJ1fVkhtj3Hw9xjLVXVYrU9QlYWrOLXBpQ6KWjbjTDTdDkoohFzgbEY=
      gitlab.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCsj2bNKTBSpIYDEGk9KxsGh3mySTRgMtXL583qmBpzeQ+jqCMRgBqB98u3z++J1sKlXHWfM9dyhSevkMwSbhoR8XIq/U0tCNyokEi/ueaBMCvbcTHhO7FcwzY92WK4Yt0aGROY5qX2UKSeOvuP4D6TPqKF1onrSzH9bx9XUf2lEdWT/ia1NEKjunUqu1xOB/StKDHMoX4/OKyIzuS0q/T1zOATthvasJFoPrAjkohTyaDUz2LN5JoH839hViyEG82yB+MjcFV5MU3N1l1QL3cVUCh93xSaua1N85qivl+siMkPGbO5xR/En4iEY6K2XPASUEMaieWVNTRCtJ4S8H+9
      # Bitbucket
      bitbucket.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIazEu89wgQZ4bqs3d63QSMzYVa0MuJ2e2gKTKqu+UUO
      bitbucket.org ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBPIQmuzMBuKdWeF4+a2sjSSpBK0iqitSQ+5BM9KhpexuGt20JpTVM7u5BDZngncgrqDMbWdxMWWOGtZ9UgbqgZE=
      bitbucket.org ssh-rsa AAAAB3NzaC1yc2EAAAABIwAAAQEAubiN81eDcafrgMeLzaFPsw2kNvEcqTKl/VqLat/MaB33pZy0y3rJZtnqwR2qOOvbwKZYKiEO1O6VqNEBxKvJJelCq0dTXWT5pbO2gDXC6h6QDXCaHo6pOHGPUy+YBaGQRGuSusMEASYiWunYN0vCAI8QaXnWMXNMdFP3jHAJH0eDsoiGnLPBlBp4TNm6rYI74nMzgz3B9IikW4WVK+dc8KZJZWYjAuORU3jc1c/NPskD2ASinf8v3xnfXeukU0sJ5N6m5E8VLjObPEO+mN2t/FZTMZLiFqPWc/ALSqnMnnhwrNi2rbfg/rd/IpL8Le3pSBne8+seeFVBoGqzHM9yXw==
      # Codeberg
      codeberg.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIVCC7Tc4A9YH1H9EZAu0CAKVOBqcPHKqgNZBF+2c5R9
      codeberg.org ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBL2pDxWr18SoiDJCGZ5LmxPygTlPu+cCKSkpqkvCyQzl5xmIMeKNdfdBpfbCGDPogb4UQQ9Ob/E1R6sxyU48oI4=
      # SourceHut
      git.sr.ht ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMZvRd4EtM7R+IHVMWmDkVU3VLQTSwQDSAvW0t2Tkj60

runcmd:
  # Phase 1: Quick setup (sync)
  - mkdir -p /opt/spuff
  - echo "pending" > /opt/spuff/bootstrap.status
  - date +%s > /tmp/spuff-last-activity
  - chmod 0644 /etc/cron.d/spuff-idle-checker

  # Configure shell for user
  - mkdir -p {{ home_dir }}/.bashrc.d
  - mkdir -p {{ home_dir }}/.config
  - mkdir -p {{ home_dir }}/.cache
  # Ensure .bashrc and .profile have the default Ubuntu content (cloud-init may not copy skeleton)
  - |
    if [ ! -s {{ home_dir }}/.bashrc ] || [ $(wc -l < {{ home_dir }}/.bashrc) -lt 10 ]; then
      cp /etc/skel/.bashrc {{ home_dir }}/.bashrc
    fi
  - |
    if [ ! -f {{ home_dir }}/.profile ]; then
      cp /etc/skel/.profile {{ home_dir }}/.profile
    fi
  - |
    if ! grep -q "bashrc.d" {{ home_dir }}/.bashrc; then
      echo '' >> {{ home_dir }}/.bashrc
      echo '# Source additional configurations' >> {{ home_dir }}/.bashrc
      echo 'for f in ~/.bashrc.d/*.sh; do [ -r "$f" ] && source "$f"; done' >> {{ home_dir }}/.bashrc
    fi
  # Fix ownership - cloud-init write_files creates as root
  - chown -R {{ username }}:{{ username }} {{ home_dir }}

  # Phase 2: Start async bootstrap
  - systemctl daemon-reload
  - systemctl enable spuff-bootstrap.service
  - systemctl start spuff-bootstrap.service --no-block

final_message: "spuff cloud-init done in $UPTIME seconds - bootstrap running async"
"#;

pub fn generate_cloud_init(
    config: &AppConfig,
    project_config: Option<&ProjectConfig>,
) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("cloud-init", CLOUD_INIT_TEMPLATE)?;

    let ssh_public_key = read_ssh_public_key(&config.ssh_key_path)?;
    let idle_timeout_seconds = config.parse_idle_timeout().as_secs();

    // Get spuff managed key (ed25519) - this avoids RSA SHA2 issues with russh
    let spuff_public_key = crate::ssh::managed_key::get_managed_public_key()
        .ok()
        .filter(|k| !k.is_empty());

    // Determine home directory based on username
    let home_dir = if config.ssh_user == "root" {
        "/root".to_string()
    } else {
        format!("/home/{}", config.ssh_user)
    };

    // Serialize project config to JSON if present
    let project_config_json = project_config
        .map(serde_json::to_string_pretty)
        .transpose()
        .map_err(|e| {
            crate::error::SpuffError::Config(format!("Failed to serialize project config: {}", e))
        })?;

    let mut context = Context::new();
    context.insert("username", &config.ssh_user);
    context.insert("home_dir", &home_dir);
    context.insert("ssh_public_key", &ssh_public_key);
    context.insert("spuff_public_key", &spuff_public_key);
    context.insert("environment", &config.environment);
    context.insert("dotfiles", &config.dotfiles);
    context.insert("idle_timeout_seconds", &idle_timeout_seconds);
    context.insert("tailscale_enabled", &config.tailscale_enabled);
    context.insert("tailscale_authkey", &config.tailscale_authkey);
    context.insert("agent_token", &config.agent_token);
    context.insert("project_config", &project_config_json);
    context.insert("has_project_config", &project_config.is_some());

    let rendered = tera.render("cloud-init", &context)?;
    Ok(rendered)
}

fn read_ssh_public_key(private_key_path: &str) -> Result<String> {
    let public_key_path = format!("{}.pub", private_key_path);

    std::fs::read_to_string(&public_key_path).map_err(|e| {
        crate::error::SpuffError::Config(format!(
            "Failed to read SSH public key '{}': {}. Make sure the key exists.",
            public_key_path, e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_ssh_key() -> (tempfile::TempDir, String) {
        let temp_dir = tempfile::tempdir().unwrap();
        let key_path = temp_dir.path().join("test_key");
        let pub_key_path = temp_dir.path().join("test_key.pub");

        std::fs::write(&key_path, "fake-private-key").unwrap();
        std::fs::write(
            &pub_key_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA... test@example.com",
        )
        .unwrap();

        (temp_dir, key_path.to_string_lossy().to_string())
    }

    #[test]
    fn test_cloud_init_contains_username() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ssh_user: "devuser".to_string(),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        assert!(result.contains("name: devuser"));
    }

    #[test]
    fn test_cloud_init_contains_ssh_key() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        assert!(result.contains("ssh-ed25519"));
        assert!(result.contains("test@example.com"));
    }

    #[test]
    fn test_cloud_init_contains_idle_timeout() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            idle_timeout: "4h".to_string(),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // 4h = 14400 seconds
        assert!(result.contains("IDLE_TIMEOUT_SECONDS=14400"));
    }

    #[test]
    fn test_cloud_init_devbox_environment() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            environment: "devbox".to_string(),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Environment is now stored in devtools.json for agent to install
        assert!(result.contains("devtools.json"));
        assert!(result.contains("\"environment\": \"devbox\""));
    }

    #[test]
    fn test_cloud_init_nix_environment() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            environment: "nix".to_string(),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Environment is now stored in devtools.json for agent to install
        assert!(result.contains("devtools.json"));
        assert!(result.contains("\"environment\": \"nix\""));
    }

    #[test]
    fn test_cloud_init_with_dotfiles() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            dotfiles: Some("https://github.com/user/dotfiles".to_string()),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Dotfiles URL is now stored in devtools.json for agent to install
        assert!(result.contains("devtools.json"));
        assert!(result.contains("\"dotfiles\": \"https://github.com/user/dotfiles\""));
    }

    #[test]
    fn test_cloud_init_without_dotfiles() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            dotfiles: None,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // When no dotfiles, devtools.json should have null
        assert!(result.contains("\"dotfiles\": null"));
    }

    #[test]
    fn test_cloud_init_tailscale_enabled() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            tailscale_enabled: true,
            tailscale_authkey: Some("tskey-abc123".to_string()),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Tailscale config is now stored in devtools.json for agent to install
        assert!(result.contains("devtools.json"));
        assert!(result.contains("\"tailscale\": true"));
        assert!(result.contains("\"tailscale_authkey\": \"tskey-abc123\""));
    }

    #[test]
    fn test_cloud_init_tailscale_disabled() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            tailscale_enabled: false,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Tailscale disabled in devtools.json
        assert!(result.contains("\"tailscale\": false"));
    }

    #[test]
    fn test_cloud_init_contains_devtools_config() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Should contain devtools.json with default config
        assert!(result.contains("devtools.json"));
        assert!(result.contains("\"docker\": true"));
        assert!(result.contains("\"shell_tools\": true"));
        assert!(result.contains("\"nodejs\": true"));
        assert!(result.contains("\"claude_code\": true"));
    }

    #[test]
    fn test_cloud_init_contains_spuff_agent() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        assert!(result.contains("spuff-agent"));
        assert!(result.contains("spuff-agent.service"));
    }

    #[test]
    fn test_cloud_init_contains_idle_checker() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        assert!(result.contains("idle-checker.sh"));
        assert!(result.contains("spuff-idle-checker"));
        assert!(result.contains("check_idle"));
    }

    #[test]
    fn test_cloud_init_devtools_nodejs_and_claude() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Node.js and Claude Code are now installed via agent, config in devtools.json
        assert!(result.contains("\"nodejs\": true"));
        assert!(result.contains("\"claude_code\": true"));
    }

    #[test]
    fn test_read_ssh_public_key_not_found() {
        let result = read_ssh_public_key("/nonexistent/key");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed to read SSH public key"));
    }

    #[test]
    fn test_parse_idle_timeout() {
        let config = AppConfig {
            idle_timeout: "2h".to_string(),
            ..Default::default()
        };
        assert_eq!(config.parse_idle_timeout().as_secs(), 7200);

        let config = AppConfig {
            idle_timeout: "30m".to_string(),
            ..Default::default()
        };
        assert_eq!(config.parse_idle_timeout().as_secs(), 1800);
    }

    #[test]
    fn test_cloud_init_is_valid_yaml() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            dotfiles: Some("https://github.com/user/dots".to_string()),
            tailscale_enabled: true,
            tailscale_authkey: Some("tskey-xxx".to_string()),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();

        // Should start with #cloud-config
        assert!(result.starts_with("#cloud-config"));

        // Basic YAML validation - should not panic
        let yaml_result: std::result::Result<serde_yaml::Value, _> = serde_yaml::from_str(&result);
        assert!(
            yaml_result.is_ok(),
            "Generated cloud-init should be valid YAML"
        );
    }

    #[test]
    fn test_cloud_init_agent_token() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            agent_token: Some("secret-agent-token-123".to_string()),
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        assert!(result.contains("agent.env"));
        assert!(result.contains("SPUFF_AGENT_TOKEN=secret-agent-token-123"));
        assert!(result.contains("permissions: '0600'"));
    }

    #[test]
    fn test_cloud_init_no_agent_token() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            agent_token: None,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();
        // Should not contain agent.env when no token is set
        assert!(!result.contains("SPUFF_AGENT_TOKEN="));
    }

    #[test]
    fn test_cloud_init_contains_git_known_hosts() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();

        // Should contain known_hosts for major git servers
        assert!(result.contains("/etc/ssh/ssh_known_hosts"));
        assert!(result.contains("github.com"));
        assert!(result.contains("gitlab.com"));
        assert!(result.contains("bitbucket.org"));
        assert!(result.contains("codeberg.org"));
        assert!(result.contains("git.sr.ht"));
    }

    #[test]
    fn test_cloud_init_async_bootstrap() {
        let (_temp_dir, key_path) = create_test_ssh_key();

        let config = AppConfig {
            ssh_key_path: key_path,
            ..Default::default()
        };

        let result = generate_cloud_init(&config, None).unwrap();

        // Should contain async bootstrap service
        assert!(result.contains("spuff-bootstrap.service"));
        assert!(result.contains("/opt/spuff/bootstrap.sh"));
        assert!(result.contains("bootstrap.status"));
        assert!(result.contains("--no-block"));
    }
}
