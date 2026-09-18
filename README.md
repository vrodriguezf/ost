# OST client := Open Source Teams client

A command-line client for Microsoft Teams written in Rust.

*Ost* means cheese in Danish.

![OST Logo](docs/logo_ost.jpg)

![TUI Screenshot](docs/tui-screenshot.svg)

## Features

- **TUI**: Interactive terminal interface with teams, channels, chats, and per-user message colors
- **Authentication**: OAuth2 device code flow for work/school and personal accounts
- **Messaging**: List chats, read messages, send messages (stable)
- **Teams**: List joined teams and channels (stable)
- **Real-time**: WebSocket connection for push notifications (Trouter)
- **Calling**: Audio and video calls with RTP/SRTP media
- **Audio** (optional): Microphone capture and speaker playback (working)
- **Video** (optional): Camera capture via V4L2 and SDL2 display (WIP)

## Status

| Feature | Status |
|---------|--------|
| TUI | Working |
| Authentication | Stable |
| Chat / Messaging | Stable |
| Teams / Channels | Stable |
| Trouter (push) | Stable |
| Audio calls | Working |
| Video calls | WIP - may cause audio issues |

**Note**: Video support is work-in-progress. Building with `--features video-capture` may interfere with audio functionality. For reliable audio calls, use `--features audio` only.

## Requirements

- Rust 1.70+ (some dependencies are pinned for compatibility with older rustc versions)
- Linux (for audio/video features)
- Nix (recommended) or manual dependency installation
- [just](https://github.com/casey/just) command runner (optional, for convenience recipes)

### Dependencies

- **Audio**: ALSA development libraries
- **Video**: V4L2, SDL2, OpenH264

## Installation

### Using Nix (recommended)

The `shell.nix` provides all required dependencies. The `just` command runner executes recipes from the `Justfile`.

```bash
nix-shell
just build
```

### Manual

Install dependencies, then:

```bash
cargo build
```

For audio support:
```bash
cargo build --features audio
```

For video support:
```bash
cargo build --features video-capture
```

For full A/V support:
```bash
cargo build --features "audio,video-capture"
```

## Usage

### TUI (Terminal User Interface)

Launch the interactive TUI for browsing teams, channels, and chats:

```bash
teams-cli tui
```

Mouse controls are enabled automatically:

- **Click** a team to expand or collapse it, or a channel/chat to open it.
- **Wheel** scrolls the sidebar, messages, search results, or debug log under the pointer, three lines at a time.
- **Click a message** to select it; click the selected message again to toggle its loaded thread replies.
- **Click compose text** to focus the editor and place the cursor. Press Enter to send.
- **Click Help** in the header or status bar to open help; click or press any key to close it.
- **Click search** in the status bar (or press Ctrl+K), then click a result to open it. Click outside the search overlay or press Esc to close it.

Keyboard navigation remains available. For terminal text selection, use your terminal's mouse-capture override (commonly Shift+drag). Mouse capture is released when OST exits.

### Live updates

Chat names remain stable when refreshes omit a title or change the latest sender.
Explicit title changes still appear. Opening an unresolved chat can recover its
label from a named participant in message history, excluding your own replies.

The TUI subscribes to incoming chat activity automatically. It refreshes the open
conversation and recent-chat list without input, and checks for missed updates
every 30 seconds. Each reconciliation reads up to 50 recent conversations and 50
messages per affected conversation; very large backlogs may need opening in Teams.
Draft text, the selected conversation, search, and your reading position survive
background refreshes. The view follows new messages when already at the bottom,
including after scrolling back down with the mouse; scrolling up keeps your place.
A successful send reveals the latest messages while keeping focus in compose.

The header reports **Live** only after push registration succeeds. **Reconnecting**
shows the retry delay while periodic checks continue; **Degraded** reports a push,
authentication, or message-refresh problem. Teams presence appears separately in
parentheses. Reconnection renews expired credentials automatically when a refresh
token is available; otherwise use `teams-cli login` in another terminal. The TUI
subscribes only to messaging and never answers calls.

### Desktop notifications

Desktop alerts use `notify-send` and your Linux desktop notification daemon. New messages in other conversations, messages received while the terminal is unfocused, and explicit mentions of your account can trigger alerts containing the sender, conversation name, and a short message preview. Ordinary messages in the focused conversation are quiet. Focus detection depends on terminal support; typing or clicking also establishes focus. Before the first focus event or interaction, OST treats the terminal as unfocused.

Your own messages, history loads, repeated deliveries, and messages more than two minutes old do not trigger alerts. Mentions use account IDs from Teams metadata; writing your display name in plain text is not a mention. Alerts wait until your account identity is available. A missing helper or notification daemon is nonfatal and logged in the debug pane. Delivery runs asynchronously with a bounded queue and timeout; excess or delayed alerts may be dropped during a burst or desktop-service stall.

To disable desktop alerts (including message previews), launch with:

```bash
OST_NOTIFICATIONS=off teams-cli tui
```

The values `0`, `false`, and `no` also disable alerts. Notifications are enabled by default.

### Unread activity

Press **u** in the sidebar to toggle the selected chat or channel read/unread, or in the messages pane to toggle the open conversation. A manual unread reminder shows a dot and survives refreshes and restarts. Marking the open chat unread keeps the reminder until you explicitly mark it read or leave and reopen it with its latest messages visible. Marking read clears currently known activity; newer incoming messages still count. This changes OST local state only, without sending Teams read receipts. In compose and search, `u` remains text input.

Unread chats and channels appear in bold with yellow badges. A number counts distinct incoming messages observed locally; a dot (`●`) means unread activity is known but its historical count is unavailable. A badge such as `2+` combines two observed messages with an unknown backlog. Collapsed teams show the combined activity of their channels. Own messages and duplicate deliveries do not add unread counts.

Initial history establishes a baseline. The native conversation read watermark can seed an unread dot, but OST does not invent a historical message count. Badges and local read horizons survive sidebar refresh, reordering, and restarts. State is separated by tenant and account under the OST configuration directory's `unread/` folder and honors `XDG_CONFIG_HOME`; saved state contains message identities and timestamps, without message text or sender names.

Apart from the explicit **u** action, OST automatically marks activity read locally only after the newest loaded content has been successfully drawn in a focused terminal. If Teams reports a newer read watermark from another client, OST also clears activity covered by that watermark while preserving newer arrivals. Opening a chat, loading messages in the background, scrolling through older messages, or covering the pane with help/search does not mark it read. Terminal focus reports are enabled automatically; a keypress or mouse interaction also confirms focus for terminals that do not report it. Such terminals cannot report a later switch to another window. This read state is local: OST does not send Teams read receipts or change another client's read state.

### Authentication

Login with device code flow:

```bash
teams-cli login
```

Force re-authentication (ignores cached token):

```bash
teams-cli login --force
```

Check authentication status:

```bash
teams-cli status
teams-cli whoami
```

### Messaging

List recent chats:

```bash
teams-cli chats
```

Read messages from a chat:

```bash
teams-cli read <chat-id> --limit 20
```

Send a message:

```bash
teams-cli send --to <chat-id> "Hello from CLI!"
```

### Teams

List joined teams and channels:

```bash
teams-cli teams
```

### Real-time Notifications

Connect to Trouter for push notifications:

```bash
teams-cli trouter
```

### Calling

Test microphone (requires `--features audio`):

```bash
teams-cli mic-test
```

Test camera (requires `--features video-capture`):

```bash
teams-cli cam-test
```

Place a test call to Echo bot:

```bash
teams-cli call-test --echo --duration 20
```

## CLI Reference

```
teams-cli [OPTIONS] <COMMAND>

Options:
  -v, --verbose  Enable debug logging

Commands:
  login      OAuth2 device code authentication
             --force    Force re-login even if cached token exists
  logout     Clear stored credentials
  status     Show token expiry status
  whoami     Verify authentication
  chats      List recent chats
             --limit N  Number of chats to show
  read       Read messages from a chat
             --limit N  Number of messages to show
  send       Send a message
             --to ID    Chat ID to send to
  teams      List joined teams and channels
  tui        Launch interactive terminal user interface
  presence   Get/set presence status
  trouter    Connect to push notification service
  call-test  Place a test call
             --echo       Call the Echo bot (call quality tester)
             --duration N Call duration in seconds (default: 30)
             --thread ID  1:1 chat thread ID to call
             --record     Enable call recording
             --camera     Enable camera capture (video-capture feature)
             --display    Enable video display window (video-capture feature)
             --tone       Use test tone instead of microphone
  mic-test   Test microphone (audio feature)
  cam-test   Test camera (video-capture feature)
```

## Testing

### Unit Tests

Run the test suite:

```bash
just test
# or
cargo test
```

Verify mouse input and terminal cleanup in a pseudo-terminal with offline fixture data:

```bash
python3 tests/tui_mouse_pty.py
```

### End-to-End Tests

E2E tests require a valid login session. Run all e2e tests:

```bash
just e2e
```

Individual e2e tests:

| Test | Description |
|------|-------------|
| `tests/e2e_trouter.sh` | Trouter WebSocket connection |
| `tests/e2e_chats.sh` | Chat listing |
| `tests/e2e_read.sh` | Message reading |
| `tests/e2e_teams.sh` | Teams/channels listing |
| `tests/e2e_echo123.sh` | Echo bot call test |

### Quality Checks

```bash
just check    # Run fmt-check, lint, and compile tests
just lint     # Run clippy lints
just fmt      # Format code
```

## Configuration

Tokens are stored in `~/.config/teams-cli/config.toml` with restricted permissions (0600).

## Documentation

- [Architecture Diagrams](docs/architecture.md) - Visual diagrams of authentication, messaging, calling, and media flows
- [Terminology Index](docs/terminology_index.md) - Glossary of protocols and terms (RTP, SRTP, ICE, SDP, etc.)
- [GUIDs Reference](docs/GUIDs.md) - Known Microsoft GUIDs (OAuth client IDs, tenant IDs, SEI UUIDs, bot MRIs)

## License

MIT License - see LICENSE file.

## Disclaimer

This is an unofficial client. Use at your own risk. Not affiliated with Microsoft.

## Related Projects

- [purple-teams](https://github.com/EionRobb/purple-teams/) - Teams plugin for libpurple (Pidgin, Finch, etc.)
