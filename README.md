# beesync-rs

A Rust application that synchronizes data from various services to Beeminder goals using a plugin-based architecture.

## Getting Started

1. Configure your Beeminder username and API key in `config.toml`
2. Uncomment and configure one or more sync modules in `config.toml`
3. Run `cargo run` (uses `config.toml`) or `cargo run -- your_config.toml`

## Supported Sync Modules

### Amazing Marvin Category Sync

Syncs completed tasks from an Amazing Marvin category to Beeminder:

- Fetches done tasks from a specified category (last 2 weeks)
- Creates a Beeminder datapoint for each completed task
- Uses Amazing Marvin task ID as unique identifier to prevent duplicates
- Task title becomes the datapoint comment

**Configuration:**
```toml
[category]
uri = { env = "AMAZING_MARVIN_URI" }
username = { env = "AMAZING_MARVIN_USERNAME" }
password = { env = "AMAZING_MARVIN_PASSWORD" }
database_name = { env = "AMAZING_MARVIN_DATABASE" }
category = "Must Do"
goal_name = "tasks"
```

### Focusmate Sync

Syncs completed Focusmate sessions to Beeminder:

- Fetches completed Focusmate sessions
- Creates a datapoint for each session with time, partner, and duration details
- Supports auto-tagging to additional goals based on hashtags in session titles
- Avoids duplicates by checking existing datapoint timestamps

**Configuration:**
```toml
[focusmate]
key = { env = "FOCUSMATE_API_KEY" }
goal_name = "focusmate"
auto_tags = ["work", "coding", "writing"]
```

### Fatebook Sync

Tracks Fatebook questions in Beeminder:

- Fetches questions from your Fatebook account
- Creates a datapoint for each new question on the "fatebook" goal
- Uses question ID as unique identifier to prevent duplicates
- Question title becomes the datapoint comment

**Configuration:**
```toml
[fatebook]
key = { env = "FATEBOOK_API_KEY" }
```

### Clean Tube Sync

Tracks YouTube viewing habits using ActivityWatch data:

- Retrieves browser window events from ActivityWatch
- Identifies YouTube videos watched longer than minimum duration
- Creates a Beeminder datapoint for each unique video
- Helps monitor and reduce YouTube consumption

**Configuration:**
```toml
[clean_tube]
activity_watch_base_url = "http://localhost:5600"
window_bucket = "aw-watcher-window_laptop"
goal_name = "youtube"
lookback_days = 7
min_video_duration_seconds = 60.0
max_datapoints = 100
```

### Clean View Sync

AI-powered social media usage tracking via ActivityWatch:

- Monitors browser window titles from ActivityWatch
- Uses OpenAI GPT to analyze titles for social media usage
- Creates binary datapoints (1 for clean days, 0 for social media usage)
- Highly customizable prompt template for AI analysis

**Configuration:**
```toml
[clean_view]
activity_watch_base_url = "http://localhost:5600"
window_bucket = "aw-watcher-window_laptop"
goal_name = "social-media"
lookback_days = 3
openai_key = { env = "OPENAI_API_KEY" }
openai_model = "gpt-4o"
min_window_duration_seconds = 10.0
prompt_template = "..."
```

### GitHub Sync

Tracks Git commits to Beeminder:

- Fetches commits from GitHub for a specified user
- Creates a datapoint for each commit with repository and commit message
- Uses commit SHA as unique identifier to prevent duplicates
- Optional authentication with GitHub personal access token for higher rate limits

**Configuration:**
```toml
[github]
key = { env = "GITHUB_TOKEN" }  # Optional - for higher rate limits
goal_name = "commits"
username = "your-github-username"
```

## API Key Configuration

The `config.toml` supports two methods for specifying API keys:

```toml
# Environment variable
service_key = { env = "SERVICE_API_KEY" }

# Command output
service_key = { cmd = "cat ~/.service_key" }
```

## License

MIT License
