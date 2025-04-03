# beesync-rs

A Rust application to sync data with Beeminder.

## Getting Started

1. Adjust your username and specify a way to access your API key in the config.
2. Uncomment and adjust one or more of the synchronization use cases in `config.toml`. 
3. Run `cargo run` (uses `config.toml`) or `cargo run -- your_config.toml`.

## Supported Sync Modules

### Clean Tube Sync

Tracks YouTube videos you've watched using ActivityWatch data:

- Retrieves window events from ActivityWatch
- Identifies YouTube videos you've watched longer than a minimum duration
- Creates a Beeminder datapoint for each unique video
- Helps track and be mindful of your YouTube consumption

### Focusmate Sync

Syncs your completed Focusmate sessions to Beeminder:

- Fetches your completed Focusmate sessions
- Creates a datapoint for each session with details like time, partner, and duration
- Supports logging to additional goals based on tags in session comment
- On first run, syncs all historical sessions if no existing datapoints are found

### Fatebook Sync

Tracks your Fatebook questions in Beeminder:

- Fetches questions from your Fatebook account
- Creates a Beeminder datapoint for each new question
- Uses the question title as the datapoint comment

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
