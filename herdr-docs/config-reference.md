# Config reference

Every canonical key Herdr reads from `config.toml`, flat and filterable. For guided setup and the reasoning behind these options, see [Configuration](/docs/configuration/).

Print the full commented default config at any time:

```bash
herdr --default-config
```

Custom command bindings (`[[keys.command]]`) are user-defined tables and are not listed per key here; see [Custom command keybindings](/docs/configuration/#custom-command-keybindings).

150 keys

## General

-   `onboarding`boolean default `unset`

    Show first-run setup on startup. Missing or true shows onboarding; continuing from onboarding writes onboarding = false.

## Theme

-   `theme.name`string default `"catppuccin"`

    Built-in theme name.

-   `theme.auto_switch`boolean default `false`

    Follow host terminal light/dark appearance and switch between theme names.

-   `theme.dark_name`string default `unset`

    Theme name used when \`auto\_switch\` selects a dark appearance.

-   `theme.light_name`string default `unset`

    Theme name used when \`auto\_switch\` selects a light appearance.

-   `theme.custom.accent`color default `unset`

    Override the accent color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.panel_bg`color default `unset`

    Override the panel\_bg color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.surface0`color default `unset`

    Override the surface0 color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.surface1`color default `unset`

    Override the surface1 color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.surface_dim`color default `unset`

    Override the surface\_dim color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.overlay0`color default `unset`

    Override the overlay0 color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.overlay1`color default `unset`

    Override the overlay1 color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.text`color default `unset`

    Override the text color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.subtext0`color default `unset`

    Override the subtext0 color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.mauve`color default `unset`

    Override the mauve color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.green`color default `unset`

    Override the green color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.yellow`color default `unset`

    Override the yellow color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.red`color default `unset`

    Override the red color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.blue`color default `unset`

    Override the blue color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.teal`color default `unset`

    Override the teal color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

-   `theme.custom.peach`color default `unset`

    Override the peach color token on top of the base theme. Accepts hex, named colors, rgb(r,g,b), or reset aliases.

## Terminal

-   `terminal.default_shell`string default `""`

    Executable used for new interactive panes. Empty means SHELL, then /bin/sh.

-   `terminal.shell_mode`enum default `"auto"`

    Startup mode for new interactive pane shells. `auto``login``non_login`

-   `terminal.new_cwd`enum default `"follow"`

    CWD policy for new interactive panes, tabs, and workspaces. `follow``home``current``path`

## Updates

-   `update.channel`enum default `"stable" ("preview" on Windows)`

    Update channel used by background version checks and herdr update. Homebrew, mise, and Nix installs ignore the preview channel. `stable``preview`

-   `update.version_check`boolean default `true`

    Check herdr.dev for new Herdr versions in the background.

-   `update.manifest_check`boolean default `true`

    Check herdr.dev for remote agent-detection manifest updates in the background. Bundled manifests and local overrides still apply.

## Keybindings

-   `keys.prefix`string default `"ctrl+b"`

    Prefix key to enter prefix mode (e.g. "ctrl+b", "f12", "esc").

-   `keys.help`keybinding default `"prefix+?"`

    Open keybinding help.

-   `keys.settings`keybinding default `"prefix+s"`

    Open settings.

-   `keys.new_workspace`keybinding default `"prefix+shift+n"`

    Create a new workspace.

-   `keys.new_worktree`keybinding default `"prefix+shift+g"`

    Create a Git worktree from the selected workspace.

-   `keys.open_worktree`keybinding default `unset`

    Open an existing Git worktree from the selected workspace. Unset by default.

-   `keys.remove_worktree`keybinding default `unset`

    Delete the selected managed worktree checkout after confirmation. Unset by default.

-   `keys.rename_workspace`keybinding default `"prefix+shift+w"`

    Rename the selected workspace.

-   `keys.close_workspace`keybinding default `"prefix+shift+d"`

    Close the selected workspace.

-   `keys.workspace_picker`keybinding default `"prefix+w"`

    Open the workspace navigation surface.

-   `keys.goto`keybinding default `"prefix+g"`

    Open the session navigator.

-   `keys.navigate_workspace_up`keybinding default `"up"`

    Move workspace selection up in navigate mode.

-   `keys.navigate_workspace_down`keybinding default `"down"`

    Move workspace selection down in navigate mode.

-   `keys.navigate_pane_left`keybinding default `"h"`

    Focus the pane to the left in navigate mode. Left arrow is always an alias.

-   `keys.navigate_pane_down`keybinding default `"j"`

    Focus the pane below in navigate mode.

-   `keys.navigate_pane_up`keybinding default `"k"`

    Focus the pane above in navigate mode.

-   `keys.navigate_pane_right`keybinding default `"l"`

    Focus the pane to the right in navigate mode. Right arrow is always an alias.

-   `keys.detach`keybinding default `"prefix+q"`

    Detach from server/client mode, or exit --no-session mode.

-   `keys.reload_config`keybinding default `"prefix+shift+r"`

    Reload config.toml in the running app/server.

-   `keys.open_notification_target`keybinding default `"prefix+o"`

    Focus the currently visible notification target.

-   `keys.previous_workspace`keybinding default `unset`

    Select the previous workspace. Unset by default.

-   `keys.next_workspace`keybinding default `unset`

    Select the next workspace. Unset by default.

-   `keys.previous_agent`keybinding default `unset`

    Focus the previous agent shown in the agent panel. Unset by default.

-   `keys.next_agent`keybinding default `unset`

    Focus the next agent shown in the agent panel. Unset by default.

-   `keys.focus_agent`keybinding default `unset`

    Focus an agent by index 1-9. Unset by default.

-   `keys.remote_image_paste`string default `"ctrl+v"`

    Local-client shortcut that sends a clipboard image to a remote Herdr session.

-   `keys.new_tab`keybinding default `"prefix+c"`

    Create a new tab in the active workspace.

-   `keys.rename_tab`keybinding default `"prefix+shift+t"`

    Rename the active tab.

-   `keys.previous_tab`keybinding default `"prefix+p"`

    Select the previous tab.

-   `keys.next_tab`keybinding default `"prefix+n"`

    Select the next tab.

-   `keys.switch_tab`keybinding default `"prefix+1..9"`

    Switch to tab 1-9.

-   `keys.switch_workspace`keybinding default `unset`

    Switch to workspace 1-9 from prefix mode. Unset by default.

-   `keys.close_tab`keybinding default `"prefix+shift+x"`

    Close the active tab.

-   `keys.rename_pane`keybinding default `"prefix+shift+p"`

    Rename the focused pane.

-   `keys.edit_scrollback`keybinding default `"prefix+e"`

    Open the focused pane scrollback in $EDITOR.

-   `keys.copy_mode`keybinding default `"prefix+["`

    Enter keyboard copy mode for the focused pane.

-   `keys.focus_pane_left`keybinding default `"prefix+h"`

    Focus the pane to the left.

-   `keys.focus_pane_down`keybinding default `"prefix+j"`

    Focus the pane below.

-   `keys.focus_pane_up`keybinding default `"prefix+k"`

    Focus the pane above.

-   `keys.focus_pane_right`keybinding default `"prefix+l"`

    Focus the pane to the right.

-   `keys.swap_pane_left`keybinding default `"prefix+shift+h"`

    Swap the focused pane with the pane to the left.

-   `keys.swap_pane_down`keybinding default `"prefix+shift+j"`

    Swap the focused pane with the pane below.

-   `keys.swap_pane_up`keybinding default `"prefix+shift+k"`

    Swap the focused pane with the pane above.

-   `keys.swap_pane_right`keybinding default `"prefix+shift+l"`

    Swap the focused pane with the pane to the right.

-   `keys.cycle_pane_next`keybinding default `"prefix+tab"`

    Cycle to the next pane.

-   `keys.cycle_pane_previous`keybinding default `"prefix+shift+tab"`

    Cycle to the previous pane.

-   `keys.last_pane`keybinding default `unset`

    Focus the last focused pane across workspaces and tabs. Unset by default.

-   `keys.split_vertical`keybinding default `"prefix+v"`

    Split pane vertically (side by side).

-   `keys.split_horizontal`keybinding default `"prefix+minus"`

    Split pane horizontally (stacked).

-   `keys.close_pane`keybinding default `"prefix+x"`

    Close the focused pane.

-   `keys.zoom`keybinding default `"prefix+z"`

    Toggle zoom for the focused pane. The legacy key name \`fullscreen\` is accepted as an alias.

-   `keys.resize_mode`keybinding default `"prefix+r"`

    Enter resize mode.

-   `keys.toggle_sidebar`keybinding default `"prefix+b"`

    Toggle sidebar collapse.

-   `keys.indexed.tabs`string default `unset`

    Modifier combo for tab shortcuts 1-9. Unset by default.

-   `keys.indexed.workspaces`string default `unset`

    Modifier combo for workspace shortcuts 1-9. Unset by default.

-   `keys.indexed.agents`string default `unset`

    Modifier combo for agent shortcuts 1-9. Unset by default.

## UI and sidebar

-   `ui.sidebar_width`integer default `26`

    Default expanded sidebar width in columns. Auto-scales based on workspace names.

-   `ui.sidebar_min_width`integer default `18`

    Minimum sidebar width (columns) when expanded.

-   `ui.sidebar_max_width`integer default `36`

    Maximum sidebar width (columns) when expanded.

-   `ui.sidebar_start_collapsed`boolean default `false`

    Start Herdr with the sidebar collapsed. Changes take effect on the next launch.

-   `ui.sidebar_collapsed_mode`enum default `compact`

    Collapsed sidebar presentation. `compact``hidden`

-   `ui.mobile_width_threshold`integer default `64`

    Terminal width at or below which Herdr uses the mobile single-column layout.

-   `ui.mouse_capture`boolean default `true`

    Capture mouse input for Herdr's mouse UI.

-   `ui.copy_on_select`boolean default `true`

    Automatically copy text selected by mouse drag or double-click. When disabled, Ctrl+C or a host-forwarded Cmd+C copies and clears the retained selection.

-   `ui.host_cursor`enum default `auto`

    Host cursor policy. `auto``native``drawn`

-   `ui.right_click_passthrough_modifier`string default `""`

    Modifier that lets right-click gestures pass through to pane apps. Empty disables it. Accepts ctrl, alt, cmd, super, meta, hyper, or a + separated combination; shift is rejected because many terminals reserve Shift+mouse.

-   `ui.redraw_on_focus_gained`boolean default `true`

    Force a full host-terminal redraw when the outer terminal regains focus.

-   `ui.mouse_scroll_lines`integer default `3`

    Lines to scroll per mouse wheel notch.

-   `ui.confirm_close`boolean default `true`

    Ask for confirmation before closing a workspace.

-   `ui.prompt_new_tab_name`boolean default `true`

    Ask for a tab name before creating a new tab.

-   `ui.prompt_new_workspace_name`boolean default `false`

    Ask for a workspace name before interactive TUI creation.

-   `ui.pane_borders`boolean default `true`

    Draw borders around split panes.

-   `ui.pane_scrollbars`boolean default `true`

    Draw interactive scrollbars beside terminal panes. Disable to reclaim the scrollbar column and keep it out of terminal-native selections.

-   `ui.pane_gaps`boolean default `true`

    Keep split panes visually separated instead of sharing divider borders.

-   `ui.show_agent_labels_on_pane_borders`boolean default `false`

    Show agent labels in split pane borders when no manual pane label is set.

-   `ui.hide_tab_bar_when_single_tab`boolean default `false`

    Hide the tab row when the workspace has one tab.

-   `ui.tab_bar_position`enum default `"top"`

    Place the desktop tab row above or below the terminal panes. `top``bottom`

-   `ui.agent_panel_sort`enum default `"spaces"`

    Agent sidebar ordering. Saved values are "spaces" or "priority"; "workspaces" is accepted as an alias for "spaces". `spaces``priority`

-   `ui.sidebar.agents.row_gap`integer default `0`

    Blank terminal rows between expanded Agent sidebar entries. Set to 1 to restore the previous spacing.

-   `ui.sidebar.agents.rows`list of token rows default `[["state_icon", "workspace", "tab"], ["agent"]]`

    Default expanded Agent sidebar layout. Entries may be token strings or inline { token, fg, bold, dim } style tables. Supports built-in and $name metadata tokens; at most 16 rows and 16 tokens per row.

-   `ui.sidebar.agents.rows_by_agent`table of token rows default `{}`

    Complete Agent-row overrides keyed by strict canonical agent id. Agents without an override use ui.sidebar.agents.rows.

-   `ui.sidebar.spaces.row_gap`integer default `0`

    Blank terminal rows between worktree groups and unrelated top-level Spaces. Worktree parents and their indented children remain packed.

-   `ui.sidebar.spaces.rows`list of token rows default `[["state_icon", "workspace"], ["branch", "git_status"]]`

    Expanded Space sidebar layout. Entries may be token strings or inline { token, fg, bold, dim } style tables. Supports built-in and $name metadata tokens; at most 16 rows and 16 tokens per row.

-   `ui.accent`color default `"cyan"`

    Accent color for highlights, borders, and navigation UI. Accepts hex (#89b4fa), named colors (cyan, blue), or RGB (rgb(137,180,250)).

## Notifications

-   `ui.toast.delivery`enum default `"off"`

    Popup notification delivery. off disables popups, herdr shows in-app toasts, terminal asks the outer terminal for a desktop notification, system asks the OS notification service directly. `off``herdr``terminal``system`

-   `ui.toast.delay_seconds`integer default `1`

    Seconds to wait before sending finished or needs-input agent notifications. Herdr notifies only if the pane is still in the same state when the delay expires. 0 is instant; valid values are 0 through 3600.

-   `ui.toast.herdr.position`enum default `"bottom-right"`

    In-app toast position, relative to the full Herdr frame. `top-left``top-right``bottom-left``bottom-right`

-   `ui.toast.clipboard.enabled`boolean default `true`

    Show the copied-to-clipboard popup after a mouse copy.

-   `ui.toast.clipboard.position`enum default `"bottom-center"`

    Copied-to-clipboard popup position. `top-left``top-center``top-right``bottom-left``bottom-center``bottom-right`

## Sound

-   `ui.sound.enabled`boolean default `true`

    Play sounds when agents change state in background workspaces.

-   `ui.sound.path`path default `unset`

    Optional mp3 file path used for all notification sounds. Relative paths are resolved from the config file's directory.

-   `ui.sound.done_path`path default `unset`

    Optional mp3 file path for "done" notifications. Relative paths are resolved from the config file's directory.

-   `ui.sound.request_path`path default `unset`

    Optional mp3 file path for "request" notifications. Relative paths are resolved from the config file's directory.

-   `ui.sound.agents.pi`enum default `"default"`

    Sound override for detected Pi agents. `default``on``off`

-   `ui.sound.agents.claude`enum default `"default"`

    Sound override for detected Claude Code agents. `default``on``off`

-   `ui.sound.agents.codex`enum default `"default"`

    Sound override for detected Codex agents. `default``on``off`

-   `ui.sound.agents.gemini`enum default `"default"`

    Sound override for detected Gemini CLI agents. `default``on``off`

-   `ui.sound.agents.cursor`enum default `"default"`

    Sound override for detected Cursor Agent CLI agents. `default``on``off`

-   `ui.sound.agents.devin`enum default `"default"`

    Sound override for detected Devin agents. `default``on``off`

-   `ui.sound.agents.agy`enum default `"default"`

    Sound override for detected Agy agents. `default``on``off`

-   `ui.sound.agents.cline`enum default `"default"`

    Sound override for detected Cline agents. `default``on``off`

-   `ui.sound.agents.open_code`enum default `"default"`

    Sound override for detected OpenCode agents. `default``on``off`

-   `ui.sound.agents.github_copilot`enum default `"default"`

    Sound override for detected GitHub Copilot CLI agents. `default``on``off`

-   `ui.sound.agents.kimi`enum default `"default"`

    Sound override for detected Kimi Code CLI agents. `default``on``off`

-   `ui.sound.agents.kiro`enum default `"default"`

    Sound override for detected Kiro agents. `default``on``off`

-   `ui.sound.agents.droid`enum default `"off"`

    Sound override for detected Droid agents. `default``on``off`

-   `ui.sound.agents.amp`enum default `"default"`

    Sound override for detected Amp agents. `default``on``off`

-   `ui.sound.agents.grok`enum default `"default"`

    Sound override for detected Grok CLI agents. `default``on``off`

-   `ui.sound.agents.hermes`enum default `"default"`

    Sound override for detected Hermes Agent agents. `default``on``off`

-   `ui.sound.agents.kilo`enum default `"default"`

    Sound override for detected Kilo Code CLI agents. `default``on``off`

-   `ui.sound.agents.qodercli`enum default `"default"`

    Sound override for detected Qoder CLI agents. `default``on``off`

-   `ui.sound.agents.maki`enum default `"default"`

    Sound override for detected Maki agents. `default``on``off`

## Session

-   `session.resume_agents_on_restore`boolean default `true`

    Resume supported AI-agent panes into their native conversation sessions when restoring a Herdr session.

## Worktrees

-   `worktrees.directory`string default `"~/.herdr/worktrees"`

    Root directory under which Herdr creates <repo>/<branch-slug> checkouts.

## Remote

-   `remote.manage_ssh_config`boolean default `true`

    Add keepalive fallbacks and private connection reuse for \`herdr --remote\`. Set false to run plain ssh unchanged.

## Advanced

-   `advanced.scrollback_limit_bytes`integer default `10000000`

    Maximum scrollback buffer size in bytes retained per pane terminal. The legacy key name \`scrollback\_lines\` is accepted as an alias.

## Experimental

-   `experimental.allow_nested`boolean default `false`

    Allow launching herdr inside an existing herdr pane.

-   `experimental.kitty_graphics`boolean default `false`

    Experimental local Kitty graphics rendering for attached clients.

-   `experimental.pane_history`boolean default `false`

    Persist pane screen history to session-history.json.

-   `experimental.reveal_hidden_cursor_for_cjk_ime`boolean default `false`

    Expose the focused pane's cursor anchor to the outer terminal even when the pane requested \`?25l\`, so macOS native input methods keep tracking the candidate window when TUIs paint their own cursor (Claude Code, pi, codex, etc.). Default: false. When the pane reports no cursor position, falls back to the pane's top-left so a stable IME anchor is always available. Trade-off when enabled: an extra hardware cursor will be visible in the outer terminal for apps that hide the cursor without painting a replacement (vim normal mode, etc.). See #149.

-   `experimental.cjk_ime_agents`list of strings default `[]`

    Restrict \`reveal\_hidden\_cursor\_for\_cjk\_ime\` to focused panes whose detected agent matches one of these names (case-insensitive). Empty list means apply to any focused pane. Unknown agent names are ignored; if the list contains no valid names, the reveal does not apply. Accepted names: pi, claude, codex, gemini, cursor, devin, cline, opencode, copilot, kimi, kiro, droid, amp, grok, hermes, kilo, qodercli, qoder, maki.

-   `experimental.cjk_ime_cursor_shape`enum default `"steady_block"`

    Cursor shape rendered for the IME anchor when \`reveal\_hidden\_cursor\_for\_cjk\_ime\` is enabled. `block``steady_block``underline``steady_underline``bar``steady_bar`

-   `experimental.switch_ascii_input_source_in_prefix`boolean default `false`

    While prefix mode is active, temporarily switch the host input source to an ASCII-capable mode so prefix commands are read as ASCII even when an IME is active, then restore the previous input source when prefix mode exits. On macOS this selects the ASCII-capable keyboard layout; on Windows it switches the IME to English (ASCII) input. Windows support is currently limited to the Korean IME; with an IME for any other language, the input source is left unchanged. macOS and Windows only; a no-op elsewhere and a best-effort no-op if the switch fails.

No keys match this filter.
