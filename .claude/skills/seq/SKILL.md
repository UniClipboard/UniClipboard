---
description: Query Seq log server for application logs. Use "setup" as argument to configure API key.
user-invocable: true
---

# Seq Log Query

Query the Seq logging server to retrieve and analyze application logs.

## Configuration

- **Default URL**: `http://localhost:5341`
- **API Key file**: `.seq-api-key` (hidden file in project root, gitignored)
- **Query script**: `.claude/skills/seq/seq-query.sh`

## Instructions

### Step 1: Check for "setup" mode

If `$ARGUMENTS` contains "setup":

1. Ask the user for their Seq API Key
2. Run the script to save the key:
   ```bash
   .claude/skills/seq/seq-query.sh --save-key "<user_provided_key>"
   ```
3. Confirm the key has been saved
4. Stop here — do not query logs

### Step 2: Parse arguments and run query

Map `$ARGUMENTS` to script options:

| User intent                                     | Script option                              |
| ----------------------------------------------- | ------------------------------------------ |
| Custom URL (e.g. `url=http://...`)              | `--url <URL>`                              |
| Filter by level (e.g. "errors", "warnings")     | `--level Error` / `--level Warning`        |
| SeqQL filter (e.g. `filter="@Level = 'Error'"`) | `--filter "<expression>"`                  |
| Text search (e.g. "search for timeout")         | `--search "timeout"`                       |
| Time range (e.g. "last hour", "since 9am")      | `--from <ISO8601>` and/or `--to <ISO8601>` |
| Signal name                                     | `--signal <name>`                          |
| Result count (e.g. "last 50")                   | `--count 50`                               |
| Raw JSON output                                 | `--raw`                                    |
| No arguments                                    | Run with defaults (latest 100 events)      |

**Time conversion**: Convert natural language time references to ISO 8601 format. Use the current date/time as reference. For example:

- "last hour" → `--from <1 hour ago in ISO 8601>`
- "today" → `--from <today 00:00:00 in ISO 8601>`

Run the query:

```bash
.claude/skills/seq/seq-query.sh [options...]
```

### Step 3: Present results

The script outputs formatted results with timestamp, level, and message for each event, plus a summary.

- If the script succeeds, present the output to the user
- Highlight any patterns you notice (recurring errors, error spikes, etc.)
- Offer suggestions for follow-up queries if relevant

### Step 4: Handle errors

The script exits with specific codes:

| Exit code | Meaning               | Action                                       |
| --------- | --------------------- | -------------------------------------------- |
| 2         | No API key configured | Tell user to run `/seq setup`                |
| 3         | Connection failed     | Seq may not be running at the specified URL  |
| 4         | Auth failed (401/403) | API key may be invalid, suggest `/seq setup` |
| 5         | Other HTTP error      | Show the error details                       |
| 6         | JSON parse error      | Show raw response for debugging              |

## Raw JSON Event Structure

When using `--raw`, events are returned as a JSON array. Each event has this structure:

```json
{
  "Timestamp": "2026-04-12T04:06:22.446550Z",
  "Level": "OK",           // "INFO", "OK", "Warning", "Error", "Fatal", "Information" (= INFO alias)
  "MessageTemplateTokens": [
    { "Text": "literal text" },
    { "PropertyName": "variable_name" }
  ],
  "Properties": [           // NOTE: array of {Name, Value} pairs, NOT a dict
    { "Name": "busy_ns", "Value": 252276041 },
    { "Name": "representation_count", "Value": "14" },
    { "Name": "code", "Value": { "file": { "path": "..." }, "line": { "number": 161 } } }
  ],
  "EventType": "$07366932",
  "SpanKind": "Internal",
  "Resource": [...],
  "Scope": [{ "Name": "name", "Value": "module::path" }],
  "Id": "event-...",
  "Links": { "Self": "...", "Group": "..." }
}
```

Key points for parsing raw JSON:
- **Properties is an array**, not a dict. Convert with: `{p['Name']: p['Value'] for p in e.get('Properties', [])}`
- Timing spans use `busy_ns` and `idle_ns` (nanoseconds). Convert to ms: `int(value) / 1e6`
- `Level: "OK"` indicates a completed tracing span (not a log level)
- `Level: "Information"` is an alias for `"INFO"` — treat them the same
- String values in Properties may be stringified even when numeric (e.g., `"14"` instead of `14`)
- Nested dict values (like `code`) contain source location info

## Example Usage

- `/seq setup` - Configure Seq API key
- `/seq` - Fetch recent 100 log events
- `/seq errors in the last hour` - Query for recent errors
- `/seq search for "connection refused"` - Full-text search
- `/seq warnings since 9am` - Warnings from this morning
- `/seq url=http://seq.example.com:5341 last 50` - Custom URL with count
- `/seq filter="@Level = 'Error' and Application = 'myapp'"` - Complex SeqQL filter
