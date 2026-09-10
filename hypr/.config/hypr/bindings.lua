-- =============================================================================
-- APP LAUNCH VARIABLES
-- =============================================================================

-- =============================================================================
-- DEFAULT BINDINGS REMOVED FOR CUSTOM OVERRIDES
-- =============================================================================
--
-- These bindings are intentionally removed before being reassigned below.
--
-- SUPER + B              -> Firefox
-- SUPER ALT + Arrows     -> Active-window resizing
-- SUPER CTRL + Arrows    -> Move window between workspaces
-- SUPER CTRL + Space     -> Wallpaper grid
-- SUPER SHIFT + G        -> Lazygit
--

hl.unbind("SUPER + B")
hl.unbind("SUPER + ALT + LEFT")
hl.unbind("SUPER + ALT + RIGHT")
hl.unbind("SUPER + ALT + UP")
hl.unbind("SUPER + ALT + DOWN")
hl.unbind("SUPER + CTRL + LEFT")
hl.unbind("SUPER + CTRL + RIGHT")
hl.unbind("SUPER + CTRL + SPACE")
hl.unbind("SUPER + SHIFT + G")
hl.unbind("ALT + TAB")
hl.unbind("ALT + SHIFT + TAB")

-- =============================================================================
-- CORE APPLICATION LAUNCHERS
-- =============================================================================

hl.bind(
    "SUPER + ALT + RETURN",
    hl.dsp.exec_cmd('uwsm-app -- xdg-terminal-exec --dir="$(omarchy-cmd-terminal-cwd)" tmux new'),
    { description = "Tmux" }
)
hl.bind(
    "SUPER + RETURN",
    hl.dsp.exec_cmd('uwsm-app -- xdg-terminal-exec --dir="$(omarchy-cmd-terminal-cwd)"'),
    { description = "Terminal" }
)

-- =============================================================================
-- BROWSERS
-- =============================================================================

hl.bind("SUPER + SHIFT + RETURN", hl.dsp.exec_cmd("omarchy-launch-browser"), { description = "Browser" })
hl.bind("SUPER + B", hl.dsp.exec_cmd("firefox --force-device-scale-factor=0.8"), { description = "Open Browser" })
hl.bind(
    "SUPER + SHIFT + ALT + B",
    hl.dsp.exec_cmd("omarchy-launch-browser --private"),
    { description = "Browser (private)" }
)

-- =============================================================================
-- FILE MANAGEMENT
-- =============================================================================

hl.bind("SUPER + SHIFT + F", hl.dsp.exec_cmd("uwsm-app -- nautilus --new-window"), { description = "File manager" })

-- This one is amazing.
hl.bind(
    "SUPER + ALT + SHIFT + F",
    hl.dsp.exec_cmd('uwsm-app -- nautilus --new-window "$(omarchy-cmd-terminal-cwd)"'),
    { description = "File manager (cwd)" }
)

-- =============================================================================
-- MEDIA
-- =============================================================================

hl.bind(
    "SUPER + SHIFT + M",
    hl.dsp.exec_cmd("~/.config/hypr/scripts/launch-or-focus spotify"),
    { description = "Music" }
)
hl.bind(
    "SUPER + SHIFT + ALT + M",
    hl.dsp.exec_cmd('~/.config/hypr/scripts/launch-or-focus org.omarchy.cliamp "omarchy-launch-tui cliamp"'),
    { description = "Music TUI" }
)

-- =============================================================================
-- DEVELOPMENT AND PRODUCTIVITY APPLICATIONS
-- =============================================================================

hl.bind("SUPER + SHIFT + N", hl.dsp.exec_cmd("omarchy-launch-editor"), { description = "Editor" })
hl.bind("SUPER + SHIFT + D", hl.dsp.exec_cmd("omarchy-launch-tui lazydocker"), { description = "Docker" })

-- =============================================================================
-- NOTES, WRITING, AND PASSWORDS
-- =============================================================================

hl.bind(
    "SUPER + SHIFT + O",
    hl.dsp.exec_cmd('~/.config/hypr/scripts/launch-or-focus ^obsidian$ "uwsm-app -- obsidian"'),
    { description = "Obsidian" }
)
hl.bind("SUPER + SHIFT + SLASH", hl.dsp.exec_cmd("uwsm-app -- 1password"), { description = "Passwords" })

-- =============================================================================
-- PRIMARY WEB APPLICATIONS
-- =============================================================================

hl.bind(
    "SUPER + SHIFT + Y",
    hl.dsp.exec_cmd('omarchy-launch-webapp "https://youtube.com/"'),
    { description = "YouTube" }
)

-- =============================================================================
-- WALLPAPER
-- =============================================================================

-- Wallpaper grid binding is supplied by background-grid.lua.

-- =============================================================================
-- ACTIVE-WINDOW RESIZING
-- =============================================================================

hl.bind(
    "SUPER + ALT + left",
    hl.dsp.window.resize({ x = -20, y = 0, relative = true }),
    { description = "Resize window left", repeating = true }
)
hl.bind(
    "SUPER + ALT + right",
    hl.dsp.window.resize({ x = 20, y = 0, relative = true }),
    { description = "Resize window right", repeating = true }
)
hl.bind(
    "SUPER + ALT + up",
    hl.dsp.window.resize({ x = 0, y = -20, relative = true }),
    { description = "Resize window up", repeating = true }
)
hl.bind(
    "SUPER + ALT + down",
    hl.dsp.window.resize({ x = 0, y = 20, relative = true }),
    { description = "Resize window down", repeating = true }
)

-- =============================================================================
-- MOVING WINDOWS BETWEEN WORKSPACES
-- =============================================================================

hl.bind(
    "SUPER + CTRL + left",
    hl.dsp.window.move({ workspace = "r-1" }),
    { description = "Move window to previous workspace" }
)
hl.bind(
    "SUPER + CTRL + right",
    hl.dsp.window.move({ workspace = "r+1" }),
    { description = "Move window to next workspace" }
)

-- =============================================================================
-- TERMINAL USER INTERFACE APPLICATIONS
-- =============================================================================

hl.bind("SUPER + SHIFT + G", hl.dsp.exec_cmd("omarchy-launch-tui lazygit"), { description = "Lazygit" })
hl.bind(
    "SUPER + SHIFT + B",
    hl.dsp.exec_cmd('~/.config/hypr/scripts/launch-or-focus org.omarchy.btop "omarchy-launch-tui btop"'),
    { description = "Activity (btop)" }
)
hl.bind(
    "SUPER + SHIFT + E",
    hl.dsp.exec_cmd('~/.config/hypr/scripts/launch-or-focus org.omarchy.aerc "omarchy-launch-tui aerc"'),
    { description = "Email (aerc)" }
)

-- =============================================================================
-- WEB APPLICATIONS AND PROJECTS
-- =============================================================================

hl.bind(
    "SUPER + SHIFT + A",
    hl.dsp.exec_cmd('omarchy-launch-webapp "https://chatgpt.com"'),
    { description = "ChatGPT" }
)
hl.bind(
    "SUPER + SHIFT + ALT + A",
    hl.dsp.exec_cmd('omarchy-launch-webapp "https://gemini.google.com/"'),
    { description = "Gemini" }
)
hl.bind(
    "SUPER + SHIFT + Z",
    hl.dsp.exec_cmd(
        'omarchy-launch-webapp "https://chatgpt.com/g/g-p-69e293a3b1e08191adb490744c113679-machine-learning-b/project"'
    ),
    { description = "Machine Learning B" }
)
hl.bind(
    "SUPER + SHIFT + X",
    hl.dsp.exec_cmd('omarchy-launch-webapp "https://chatgpt.com/g/g-p-698420698d708191aa845c44f3f7008a-orel/project"'),
    { description = "Online Reinforcement Learning GPT" }
)

-- =============================================================================
-- SPECIAL WORKSPACES AND SCRATCHPADS
-- =============================================================================

hl.bind("SUPER + H", hl.dsp.workspace.toggle_special("hidden"), { description = "Toggle Special Workspace" })
hl.bind(
    "SUPER + SHIFT + H",
    hl.dsp.window.move({ workspace = "special:hidden" }),
    { description = "Move Window to Special Workspace" }
)
hl.bind(
    "SUPER + SHIFT + S",
    hl.dsp.window.move({ workspace = "special:scratchpad" }),
    { description = "Move Window to Special Workspace" }
)

-- =============================================================================
-- CHAT APPLICATIONS
-- =============================================================================

hl.bind("SUPER + ALT + D", hl.dsp.exec_cmd("flatpak run com.discordapp.Discord"), { description = "Launch Discord" })

-- =============================================================================
-- DICTATION
-- =============================================================================

-- Override Omarchy's default voxtype toggle.
hl.unbind("SUPER + CTRL + X")

hl.bind("SUPER + D", hl.dsp.exec_cmd("~/.local/bin/dictate batch toggle"), { description = "Dictation (batch toggle)" })
hl.bind(
    "SUPER + CTRL + SHIFT + X",
    hl.dsp.exec_cmd("~/.local/bin/dictate cancel"),
    { description = "Dictation cancel" }
)

-- =============================================================================
-- HYPRLAND CONFIGURATION AND VISUAL EFFECTS
-- =============================================================================

hl.bind("SUPER + SHIFT + R", hl.dsp.exec_cmd("hyprctl reload"), { description = "Reload Hyprland configuration" })
hl.bind("SUPER + ALT + B", require("actions").toggle_blur, { description = "Toggle blur" })
hl.bind(
    "SUPER + ALT + T",
    hl.dsp.exec_cmd("~/.config/hypr/scripts/toggle-transparency"),
    { description = "Toggle transparency globally" }
)

-- =============================================================================
-- NOTIFICATIONS
-- =============================================================================

hl.bind("SUPER + N", hl.dsp.exec_cmd("~/.local/bin/notification-menu"), { description = "Notification menu" })
hl.bind("SUPER + ALT + Comma", hl.dsp.exec_cmd("makoctl invoke"), { description = "Invoke last notification" })

-- =============================================================================
-- TERMINAL PICKERS AND GLOBAL UTILITIES
-- =============================================================================

hl.bind("SUPER + Z", hl.dsp.exec_cmd("ghostty --class=pdf.picker -e ~/.local/bin/pdf-picker"))
hl.bind("SUPER + ALT + PERIOD", hl.dsp.exec_cmd("ghostty --class=global.snip -e ~/.local/bin/globalsnip"))
hl.bind("SUPER + ALT + M", hl.dsp.exec_cmd("ghostty --class=macro.picker -e ~/.local/bin/macro-picker"))
