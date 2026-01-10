# 🦝 gcp-racoon

**GCP Log Spy with Gemini AI**

A powerful terminal UI for streaming, querying, and analyzing Google Cloud Platform logs with Gemini powered insights.

![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)

---

## ✨ Features

- 🔴 **Real-time Log Streaming** - Watch logs as they happen with a beautiful TUI
- 🔍 **Powerful Querying** - Use GCP filter syntax or natural language
- 🤖 **AI-Powered Analysis** - Explain errors, generate queries, answer questions about your logs
- 📁 **Offline Mode** - Export logs and analyze them later without internet
- 🎨 **Themeable** - Dark and light themes for comfortable viewing
- ⌨️ **Vim-style Navigation** - Efficient keyboard-driven interface
- 🔒 **Secure** - Uses your existing GCP credentials

---

## 📦 Installation

### From Binary (Recommended)

Download the latest release for your platform:

```bash
# macOS (Apple Silicon)
curl -L https://github.com/YOUR_USERNAME/racoon/releases/latest/download/racoon-aarch64-apple-darwin.tar.gz | tar xz
sudo mv racoon /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/YOUR_USERNAME/racoon/releases/latest/download/racoon-x86_64-apple-darwin.tar.gz | tar xz
sudo mv racoon /usr/local/bin/

# Linux (x86_64)
curl -L https://github.com/YOUR_USERNAME/racoon/releases/latest/download/racoon-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv racoon /usr/local/bin/

# Windows (PowerShell)
Invoke-WebRequest -Uri https://github.com/YOUR_USERNAME/racoon/releases/latest/download/racoon-x86_64-pc-windows-msvc.zip -OutFile racoon.zip
Expand-Archive racoon.zip -DestinationPath .
Move-Item racoon.exe C:\Windows\System32\
```

### From Source

```bash
# Clone the repository
git clone https://github.com/YOUR_USERNAME/racoon.git
cd racoon

# Build and install
cargo install --path .
```

### Using Cargo

```bash
cargo install racoon
```

### Using Docker

```bash
# Pull the image
docker pull YOUR_USERNAME/racoon:latest

# Run directly
docker run --rm -it \
    -v ~/.config/gcloud:/root/.config/gcloud:ro \
    -e GEMINI_API_KEY \
    YOUR_USERNAME/racoon tail --project my-project

# Create an alias for convenience
alias racoon='docker run --rm -it \
    -v ~/.config/gcloud:/root/.config/gcloud:ro \
    -v ~/.config/racoon:/root/.config/racoon:ro \
    -e GEMINI_API_KEY \
    YOUR_USERNAME/racoon'

# Now use like a normal command
racoon --help
```

### Using Homebrew (macOS/Linux)

```bash
brew tap YOUR_USERNAME/tap
brew install racoon
```

---

## 🚀 Quick Start

### 1. Authenticate with GCP

Racoon uses your existing GCP credentials. Make sure you're authenticated:

```bash
# Using gcloud
gcloud auth application-default login

# Or set service account key
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/key.json
```

### 2. Set up AI (Optional)

For AI features, set your Gemini API key:

```bash
export GEMINI_API_KEY=your-api-key-here
```

Or add to config file (`~/.config/racoon/config.toml`):

```toml
[ai]
api_key = "your-api-key-here"
```

### 3. Start Using Racoon

```bash
# Stream logs in real-time
racoon tail --project your-project-id

# Query logs
racoon query --project your-project-id --filter 'severity>=ERROR'

# List projects
racoon projects

# Check authentication
racoon auth
```

---

## 📖 Usage

### Commands

| Command | Description |
|---------|-------------|
| `tail` | Stream logs in real-time with TUI |
| `query` | Execute a one-off log query |
| `projects` | List accessible GCP projects |
| `auth` | Verify authentication status |
| `export` | Export logs to a file |
| `download` | Quick download logs |
| `view` | View logs from a local file (offline) |
| `config` | Manage configuration |

### Tail Mode (TUI)

```bash
racoon tail --project my-project --since 1h --severity WARNING
```

#### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `g` / `Home` | Go to first log |
| `G` / `End` | Go to last log |
| `PageDown` | Skip 10 logs down |
| `PageUp` | Skip 10 logs up |
| `/` | Search within logs |
| `n` | Next search match |
| `N` | Previous search match |
| `f` | Set GCP filter |
| `e` | Explain selected log with AI |
| `?` | Ask AI a question |
| `p` | Pause/resume streaming |
| `t` | Toggle theme (dark/light) |
| `Ctrl+j/k` | Scroll detail pane |
| `Ctrl+n/p` | Scroll AI pane |
| `Esc` | Clear search/AI/filter |
| `q` | Quit |

### Query Mode

```bash
# Basic query
racoon query --project my-project --since 2h --limit 50

# With filter
racoon query --project my-project --filter 'resource.type="cloud_run_revision"'

# With AI-generated filter
racoon query --project my-project --ai "all errors from Cloud Run today"
```

### Export & Offline Mode

```bash
# Export logs to JSON
racoon export --project my-project --output logs.json --limit 1000

# Export with AI query
racoon export --project my-project --output errors.json --ai "critical errors last 24h"

# Export to NDJSON (newline-delimited)
racoon export --project my-project --output logs.ndjson --format ndjson

# View exported logs offline
racoon view logs.json
```

### Configuration

```bash
# Show config file path
racoon config path

# Create default config
racoon config init

# Show current config
racoon config show

# Edit config
racoon config edit
```

---

## ⚙️ Configuration

Config file location: `~/.config/racoon/config.toml`

```toml
# AI Settings
[ai]
api_key = "your-gemini-api-key"
model = "gemini-2.0-flash"

# Default Values
[defaults]
project = "my-default-project"
since = "1h"
limit = 100
severity = "INFO"

# UI Settings
[ui]
max_logs = 100
mouse = false
theme = "dark"  # or "light"

# Logging
[logging]
level = "info"
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `GEMINI_API_KEY` | Gemini API key (overrides config) |
| `GOOGLE_APPLICATION_CREDENTIALS` | Path to GCP service account key |

---

## 🤖 AI Features

Racoon integrates with Google Gemini for intelligent log analysis.

### Explain Logs

Press `e` on any log to get an AI explanation:

```
┌─ AI Analysis ────────────────────────────────────────┐
│ ### 1. Summary                                       │
│ This error indicates a connection timeout to the     │
│ database server. The request waited 30 seconds but   │
│ no response was received.                            │
│                                                      │
│ ### 2. Severity Assessment                           │
│ **High** - Database connectivity issues can cause    │
│ cascading failures across dependent services.        │
│                                                      │
│ ### 3. Recommended Actions                           │
│ 1. Check database server health                      │
│ 2. Verify network connectivity                       │
│ 3. Review connection pool settings                   │
└──────────────────────────────────────────────────────┘
```

### Ask Questions

Press `?` and ask in natural language:

```
? Are there any errors from Cloud Run in the last hour?

┌─ AI Analysis ────────────────────────────────────────┐
│ Yes, I found 3 errors from Cloud Run:                │
│                                                      │
│ 1. 10:23:15 - Container failed to start (OOM)        │
│ 2. 10:25:33 - Request timeout after 300s             │
│ 3. 10:45:01 - Connection refused to backend          │
│                                                      │
│ The most critical is the OOM error which suggests    │
│ your container needs more memory allocation.         │
└──────────────────────────────────────────────────────┘
```

### AI Commands

You can also use AI to set filters:

```
? 帮我设置过滤条件为仅显示ERROR以上的日志

✅ 已设置过滤条件: severity>=ERROR
```

---

## 🌐 Supported GCP Log Types

- Cloud Run
- Cloud Functions
- App Engine
- Kubernetes Engine (GKE)
- Compute Engine
- Cloud SQL
- BigQuery
- Custom logs (any `logName`)

---

## 🔧 Troubleshooting

### Authentication Issues

```bash
# Re-authenticate
gcloud auth application-default login

# Check current credentials
racoon auth
```

### No Logs Showing

1. Check project ID is correct
2. Verify you have `roles/logging.viewer` permission
3. Try a broader time range: `--since 24h`

### AI Not Working

1. Check API key is set: `racoon config show`
2. Verify API key at [Google AI Studio](https://makersuite.google.com/app/apikey)
3. Check network connectivity

---

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

## 🙏 Acknowledgments

- [ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework
- [Google Cloud Logging](https://cloud.google.com/logging) - Log infrastructure
- [Google Gemini](https://ai.google.dev/) - AI capabilities

---

Made with ❤️ and 🦝