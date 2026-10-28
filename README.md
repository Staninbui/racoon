# 🦝 gcp-racoon

**GCP Log Spy with Gemini AI**

A powerful terminal UI for streaming, querying, and analyzing Google Cloud Platform logs with Gemini powered insights.

![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)

---

## ✨ Features

- 🔴 **Real-time Log Streaming** - Watch logs as they happen with a beautiful TUI
- 🚨 **Error Fingerprinting & Spike Radar (`c` key)** - Cluster repetitive error storms into canonical patterns with real-time Unicode velocity sparklines (`▁▂▃▄▅▆▇█`)
- 🕵️ **Surrounding Instance Context (`s` key)** - Inspect ±5s of `INFO`/`DEBUG` logs on the exact same Pod / Cloud Run Revision / VM instance right before and after an error (even without a `trace_id`)
- 🌐 **Multi-Project & Log View Streaming** - Monitor entire microservice chains across projects (`--project prod-apigw,prod-order,prod-payment`) or centralized Log Bucket Views in a single TUI with `[project]` badges
- 🔗 **Distributed Trace Context (`x` / `E` keys)** - Isolate full request traces across microservices and run 1-keystroke Full-Trace / Context AI Root Cause Analysis (RCA)
- 🪙 **Client-Side AI Token Optimizer** - Cuts LLM token consumption by 80%–98% via semantic JSON slimming, stacktrace folding, Shared Context extraction, RLE duplicate-pattern folding, and instant 0-token fingerprint LRU caching
- 🔍 **Powerful Querying** - Use GCP filter syntax or natural language
- 🤖 **Multi-Provider AI Analysis (`m` key)** - Switch seamlessly between Google Gemini (`gemini-3.8-flash`, `gemini-4-argon`), OpenAI, Anthropic, and Agnes AI
- 🛡️ **Pre-AI PII & Secret Guard** - Automatically scrubs Bearer tokens, JWTs, API keys, URI passwords, Luhn credit cards, and customer emails locally before sending logs to any LLM
- 📋 **On-Call Productivity Shortcuts (`y` / `Y` / `o` keys)** - 1-keystroke copy of log JSON or AI RCA Markdown to clipboard, deep-link jump to Google Cloud Logs Explorer in browser, and `↑`/`↓` command input history
- 📁 **Offline Mode** - Export logs (paginated JSON or streaming NDJSON) and analyze them later without internet
- 🎨 **Themeable** - Dark and light themes for comfortable viewing
- ⌨️ **Vim-style Navigation** - Efficient keyboard-driven interface
- 🔒 **Secure** - Uses your existing GCP credentials

---

## 📦 Installation

### From Binary (Recommended)

Download the latest release for your platform:

```bash
# macOS (Apple Silicon)
curl -L https://github.com/Staninbui/racoon/releases/latest/download/gcp-racoon-aarch64-apple-darwin.tar.gz | tar xz
sudo mv gcp-racoon /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/Staninbui/racoon/releases/latest/download/gcp-racoon-x86_64-apple-darwin.tar.gz | tar xz
sudo mv gcp-racoon /usr/local/bin/

# Linux (x86_64)
curl -L https://github.com/Staninbui/racoon/releases/latest/download/gcp-racoon-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv gcp-racoon /usr/local/bin/

# Windows (PowerShell)
Invoke-WebRequest -Uri https://github.com/Staninbui/racoon/releases/latest/download/gcp-racoon-x86_64-pc-windows-msvc.zip -OutFile gcp-racoon.zip
Expand-Archive gcp-racoon.zip -DestinationPath .
Move-Item gcp-racoon.exe C:\Windows\System32\
```

### From Source

```bash
# Clone the repository
git clone https://github.com/Staninbui/racoon.git
cd racoon

# Build and install
cargo install --path .
```

### Using Cargo

```bash
cargo install gcp-racoon
```

### Using Docker

```bash
# Pull the image
docker pull ghcr.io/staninbui/gcp-racoon:latest

# Run directly
docker run --rm -it \
    -v ~/.config/gcloud:/root/.config/gcloud:ro \
    -e GEMINI_API_KEY \
    ghcr.io/staninbui/gcp-racoon tail --project my-project

# Create an alias for convenience
alias racoon='docker run --rm -it \
    -v ~/.config/gcloud:/root/.config/gcloud:ro \
    -v ~/.config/racoon:/root/.config/racoon:ro \
    -e GEMINI_API_KEY \
    ghcr.io/staninbui/gcp-racoon'

# Now use like a normal command
racoon --help
```

### Using Homebrew (macOS/Linux)

```bash
# Coming soon
# brew tap staninbui/tap
# brew install gcp-racoon
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

### 2. Set up AI (Zero-Key Vertex AI ADC or External API Key)

**Option A: Enterprise Zero-API-Key Mode (Google Cloud Vertex AI via ADC / IAM)**

No external API key needed! Racoon reuses your GCP Application Default Credentials (and respects `--impersonate-service-account`) to call Vertex AI within your GCP project boundary:

```toml
# ~/.config/racoon/config.toml
[ai]
provider = "vertex"
model = "gemini-3.8-flash"

[ai.providers.vertex]
location = "us-central1" # or "global"
```

**Option B: External Provider API Key (Google AI Studio, OpenAI, Anthropic, Agnes AI)**

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

# Least-privilege production access via IAM Service Account Impersonation
racoon --impersonate-service-account sre-reader@prod-proj.iam.gserviceaccount.com tail --project prod-proj

# Query logs
racoon query --project your-project-id --filter 'severity>=ERROR'

# List projects
racoon projects

# Check authentication (supports --impersonate-service-account)
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
| `view` | View logs from a local file or `stdin` (`racoon view -`, supports JSON, NDJSON & plain text) |
| `diff` | Compare error fingerprints between two time windows or log files to detect release regressions |
| `models` | Interactively select, list, or update AI models (Google, OpenAI, Anthropic, Agnes AI) |
| `update` | Check for racoon updates and overwrite/refresh the AI models registry |
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
| `Tab` / `Shift+Tab` | Cycle keyboard focus across **Logs**, **Detail**, and **AI** panes |
| `z` | Toggle **Full-Screen Maximize** for the currently focused pane (`Esc` to restore) |
| `b` | Cycle **Saved Filter Bookmarks** (`[bookmarks]` in `config.toml` + built-in SRE presets) |
| `c` | Toggle **Error Fingerprint & Spike Radar** (`Enter` to drill into selected cluster) |
| `-` | **Mute / Exclude** the selected noisy log pattern (`-fingerprint:"..."`, `Esc` to restore) |
| `s` | Toggle **Surrounding Logs Context** on the same resource instance (`🎯` marks anchor log) |
| `[` / `]` | Shrink / Expand Surrounding Context time window (`±5s` ↔ `±15s` ↔ `±30s` ↔ `±60s` ↔ `±300s`) |
| `x` | Toggle **Distributed Trace Context** (isolate request by `trace`, `spanId`, or `request_id`) |
| `e` | Explain selected log (or selected Error Cluster) with AI |
| `E` (`Shift+e`) | Run **Full-Trace / Surrounding Context AI Root Cause Analysis (RCA)** |
| `w` | Save a self-contained **Markdown Incident Snapshot** (`incident-<timestamp>.md`) |
| `y` | Copy selected log JSON (or Error Cluster summary + JSON) to clipboard |
| `Y` (`Shift+y`) | Copy AI analysis / RCA Markdown report to clipboard |
| `o` | Open selected log entry in **Google Cloud Logs Explorer** in browser |
| `?` | Ask AI a question, **follow up on the active AI diagnosis**, or set filters in natural language |
| `m` | Open interactive AI Provider & Model selector |
| `/` | Search within logs (`↑` / `↓` cycles persistent history at `~/.config/racoon/history.json`) |
| `n` | Next search match |
| `N` | Previous search match |
| `f` | Set GCP filter (`↑` / `↓` cycles persistent history at `~/.config/racoon/history.json`) |
| `p` | Pause/resume streaming |
| `t` | Toggle theme (dark/light) |
| `Ctrl+j/k` | Scroll detail pane |
| `Ctrl+n/p` | Scroll AI pane |
| `Esc` | Restore maximized pane / exit Radar / exit Trace or Mute mode / clear search, AI response, or filter |
| `q` | Quit |

### Multi-Provider AI Models & Self-Update

```bash
# Launch interactive two-step selector (Step 1: Provider -> Step 2: Model)
racoon models

# List all supported providers and cached models
racoon models list

# Manually sync latest models from provider APIs
racoon models update

# Check for racoon version updates and overwrite/refresh the models registry
racoon update
```

### Query Mode

```bash
# Basic query
racoon query --project my-project --since 2h --limit 50

# With filter
racoon query --project my-project --filter 'resource.type="cloud_run_revision"'

# Historical time-range query (--from / --to)
racoon query --project my-project --from "2026-10-27 14:00:00" --to "2026-10-27 14:30:00"

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

# View exported logs offline (opens interactive TUI by default, or use --no-tui)
racoon view logs.json

# Pipe logs from stdin (kubectl, docker, gcloud, or plain text)
kubectl logs deploy/payment-api --tail=200 | racoon view - --no-tui

# Compare error fingerprints before and after a deployment (GCP windows or local files)
racoon diff --project my-project --baseline 2h..1h --current 1h..now --ai
racoon diff --baseline-file before.json --current-file after.json
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
# AI Settings (Multi-Provider: google, openai, anthropic, agnes)
[ai]
provider = "google"
model = "gemini-3.8-flash" # or "gemini-4-argon"

[ai.providers.google]
api_key = "your-gemini-api-key"

[ai.providers.openai]
api_key = "your-openai-api-key"

[ai.providers.anthropic]
api_key = "your-anthropic-api-key"

[ai.providers.agnes]
api_key = "your-agnes-api-key"
base_url = "https://apihub.agnes-ai.com/v1"

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
| `GEMINI_API_KEY` | Google Gemini API key |
| `OPENAI_API_KEY` | OpenAI GPT API key |
| `ANTHROPIC_API_KEY` | Anthropic Claude API key |
| `AGNES_API_KEY` | Agnes AI API key |
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