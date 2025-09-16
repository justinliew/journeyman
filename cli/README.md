# NHL Player Database Generator

A command-line tool to generate a comprehensive NHL player database by fetching roster data from the NHL API.

## Features

- **Async HTTP requests** with proper rate limiting to avoid being throttled
- **Configurable time ranges** - specify which seasons to collect data for
- **Rate limiting** - customizable delay between API requests
- **Progress tracking** - see real-time progress as data is collected
- **Deduplication** - automatically removes duplicate player entries
- **JSON output** - generates a clean, structured JSON file

## Installation

```bash
cd cli
cargo build --release
```

## Usage

### Basic usage (recent seasons only)
```bash
cargo run -- --output players.json
```

### Full historical data (1932-present)
```bash
cargo run -- --start-year 1932 --end-year 2025 --output full_nhl_history.json --delay 150
```

### Command-line options

- `--output, -o`: Output JSON file path (default: `nhl_players.json`)
- `--delay, -d`: Delay between requests in milliseconds (default: 100ms)
- `--start-year`: First season start year (default: 2015)
- `--end-year`: Last season start year (default: 2025)

### Examples

```bash
# Generate database for last 10 seasons with 200ms delay
cargo run -- --start-year 2015 --delay 200

# Generate complete historical database (will take a while!)
cargo run -- --start-year 1932 --end-year 2025 --delay 150 --output complete_nhl_history.json

# Quick recent data for testing
cargo run -- --start-year 2023 --end-year 2025 --delay 50 --output recent_players.json
```

## Output Format

The generated JSON file contains:

```json
{
  "teams": {
    "BOS": ["Player Name 1", "Player Name 2", ...],
    "TOR": ["Player Name 1", "Player Name 2", ...],
    ...
  },
  "generated_at": "2025-09-15T12:34:56.789Z",
  "seasons_covered": ["20232024", "20242025", ...]
}
```

## Rate Limiting

The tool includes built-in rate limiting to be respectful to the NHL API:

- Default: 100ms delay between requests
- Recommended for full historical data: 150-200ms delay
- For recent data only: 50-100ms delay is usually fine

**Note**: Collecting full historical data (1932-2025) will make ~3,000 API requests and take 5-10 minutes depending on your delay setting.

## Error Handling

- Failed requests are logged but don't stop the collection process
- Network timeouts are handled gracefully (30-second timeout per request)
- Invalid/missing data is skipped without crashing

## Tips

1. **Start small**: Test with recent seasons first (`--start-year 2023`)
2. **Be patient**: Full historical collection takes time due to rate limiting
3. **Monitor progress**: The tool shows real-time progress and statistics
4. **Adjust delay**: Increase delay if you get rate-limited by the API
