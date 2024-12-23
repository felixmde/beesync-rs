# beesync-rs

A Rust application to sync data with Beeminder using ActivityWatch and
Focusmate.

## Features

- Fetch and log YouTube events from ActivityWatch.
- Sync Focusmate sessions to Beeminder.
- Automatic tagging for Focusmate sessions.

## Usage

1. Set environment variables `BEEMINDER_API_KEY` and `FOCUSMATE_API_KEY`.
2. Configure `config.toml` with your Beeminder and ActivityWatch settings.
3. Run the application:

```bash
cargo run -- config.toml
```

**Note:** When performing a Focusmate sync for the first time, if the script
does not detect an existing data point with a value of `1.0`, it will sync all
your sessions from the beginning of time.

## License

MIT License
