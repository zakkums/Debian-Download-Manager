# DDM — Debian Download Manager

High-throughput segmented download manager with resume, parallel jobs, and a simple queue.

## Install

```bash
cargo build --release -p ddm-cli
# Binary: target/release/ddm
```

Requires Rust (stable) and system libcurl. On Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libcurl4-openssl-dev
```

## Quick start

```bash
# Add a job (saved to current directory by default)
ddm add https://example.com/large.iso

# Optional: save to a specific directory (stored with the job for resume)
ddm add https://example.com/large.iso --download-dir /data/downloads

# Process the queue (run one job at a time by default)
ddm run

# Run up to 4 jobs in parallel
ddm run --jobs 4

# Overwrite if the final file already exists
ddm run --overwrite
```

## Commands

| Command | Description |
|--------|-------------|
| `ddm add <URL>` | Add a download job (optionally `--download-dir DIR`) |
| `ddm run` | Process queued jobs; supports `--jobs N`, `--force-restart`, `--overwrite` |
| `ddm status` | List all jobs and their state |
| `ddm pause <id>` | Pause a job; if `ddm run` is active, stops that job within ~1s and saves progress |
| `ddm resume <id>` | Set a paused job back to queued |
| `ddm remove <id>` | Remove job from DB; use `--delete-files` to remove .part and final file |
| `ddm import-har <path>` | Create jobs from a HAR file |
| `ddm bench <URL>` | Benchmark segment counts for a URL |
| `ddm checksum <path>` | Print SHA-256 of a file |
| `ddm completions <shell>` | Print shell completion script (bash, zsh, fish, etc.) |
| `ddm manpage` | Print man page (e.g. `ddm manpage > share/man/man1/ddm.1`) |

## Configuration

Config file: **`~/.config/ddm/config.toml`** (created with defaults on first run).

| Option | Default | Description |
|--------|---------|-------------|
| `max_total_connections` | 64 | Global connection limit across all jobs |
| `max_connections_per_host` | 16 | Connections per host per job |
| `min_segments` | 4 | Minimum segments per file |
| `max_segments` | 16 | Maximum segments per file |
| `max_bytes_per_sec` | (none) | Optional global bandwidth cap |
| `segment_buffer_bytes` | (none) | Optional buffer size per segment |
| `download_backend` | `"easy"` | `"easy"` (threads) or `"multi"` (curl multi) |
| `[retry]` | (built-in) | Optional `max_attempts`, `base_delay_secs`, `max_delay_secs` |

Example `config.toml`:

```toml
max_total_connections = 32
max_connections_per_host = 8
max_segments = 8
download_backend = "multi"
```

State (DB, logs, control socket): **`~/.local/state/ddm/`**

## Resume and pause

- Each job stores its **download directory**; you can run `ddm run` from any directory and resume works.
- **Pause** sets the job to Paused and, if a run is active, signals it to stop within about a second; progress is saved.
- **Resume** sets the job back to Queued; the next `ddm run` continues from the saved bitmap.

## License

MIT OR Apache-2.0. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
