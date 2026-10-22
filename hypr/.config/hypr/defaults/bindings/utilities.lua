-- Menus
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("desktopctl launcher"), { description = "Launch apps" })
hl.bind("SUPER + CTRL + E", hl.dsp.exec_cmd("desktopctl launcher -m symbols"), { description = "Emoji picker" })
hl.bind("SUPER + CTRL + C", hl.dsp.exec_cmd("desktopctl menu capture"), { description = "Capture menu" })
hl.bind("SUPER + CTRL + O", hl.dsp.exec_cmd("desktopctl menu toggle"), { description = "Toggle menu" })
hl.bind("SUPER + CTRL + H", hl.dsp.exec_cmd("desktopctl menu hardware"), { description = "Hardware menu" })
hl.bind("SUPER + ALT + SPACE", hl.dsp.exec_cmd("desktopctl menu"), { description = "Desktop menu" })
hl.bind("SUPER + SHIFT + code:201", hl.dsp.exec_cmd("desktopctl menu"), { description = "Desktop menu" })
hl.bind("SUPER + ESCAPE", hl.dsp.exec_cmd("desktopctl menu system"), { description = "System menu" })
hl.bind("XF86PowerOff", hl.dsp.exec_cmd("desktopctl menu system"), { description = "Power menu", locked = true })
hl.bind("SUPER + K", hl.dsp.exec_cmd("desktopctl keybindings"), { description = "Show key bindings" })
hl.bind("XF86Calculator", hl.dsp.exec_cmd("gnome-calculator"), { description = "Calculator" })

-- Aesthetics
hl.bind("SUPER + SHIFT + SPACE", hl.dsp.exec_cmd("desktopctl toggle bar"), { description = "Toggle top bar" })
hl.bind("SUPER + CTRL + SPACE", hl.dsp.exec_cmd("desktopctl menu background"), { description = "Theme background menu" })
hl.bind("SUPER + SHIFT + CTRL + SPACE", hl.dsp.exec_cmd("desktopctl menu theme"), { description = "Theme menu" })
hl.bind(
    "SUPER + BACKSPACE",
    hl.dsp.window.set_prop({ prop = "opaque", value = "toggle" }),
    { description = "Toggle window transparency" }
)
hl.bind(
    "SUPER + SHIFT + BACKSPACE",
    hl.dsp.exec_cmd("desktopctl toggle gaps"),
    { description = "Toggle window gaps" }
)
hl.bind(
    "SUPER + CTRL + BACKSPACE",
    hl.dsp.exec_cmd("desktopctl toggle square"),
    { description = "Toggle single-window square aspect" }
)

-- Notifications
hl.bind("SUPER + COMMA", hl.dsp.exec_cmd("makoctl dismiss"), { description = "Dismiss last notification" })
hl.bind(
    "SUPER + SHIFT + COMMA",
    hl.dsp.exec_cmd("makoctl dismiss --all"),
    { description = "Dismiss all notifications" }
)
hl.bind(
    "SUPER + CTRL + COMMA",
    hl.dsp.exec_cmd("desktopctl toggle notifications"),
    { description = "Toggle silencing notifications" }
)
hl.bind("SUPER + ALT + COMMA", hl.dsp.exec_cmd("makoctl invoke"), { description = "Invoke last notification" })
hl.bind(
    "SUPER + SHIFT + ALT + COMMA",
    hl.dsp.exec_cmd("makoctl restore"),
    { description = "Restore last notification" }
)

-- Toggles
hl.bind("SUPER + CTRL + I", hl.dsp.exec_cmd("desktopctl toggle idle"), { description = "Toggle locking on idle" })
hl.bind("SUPER + CTRL + N", hl.dsp.exec_cmd("desktopctl toggle nightlight"), { description = "Toggle nightlight" })
hl.bind(
    "SUPER + CTRL + Delete",
    hl.dsp.exec_cmd("desktopctl display internal toggle"),
    { description = "Toggle laptop display" }
)
hl.bind(
    "SUPER + CTRL + ALT + Delete",
    hl.dsp.exec_cmd("desktopctl display mirror toggle"),
    { description = "Toggle laptop display mirroring" }
)
hl.bind(
    "switch:on:Lid Switch",
    hl.dsp.exec_cmd("desktopctl display external && desktopctl display internal off"),
    { locked = true }
)
hl.bind("switch:off:Lid Switch", hl.dsp.exec_cmd("desktopctl display internal on"), { locked = true })

-- Captures
hl.bind("PRINT", hl.dsp.exec_cmd("desktopctl capture screenshot"), { description = "Screenshot" })
hl.bind("ALT + PRINT", hl.dsp.exec_cmd("desktopctl menu screenrecord"), { description = "Screenrecording" })
hl.bind("SUPER + PRINT", hl.dsp.exec_cmd("pkill hyprpicker || hyprpicker -a"), { description = "Color picker" })
hl.bind(
    "SUPER + CTRL + PRINT",
    hl.dsp.exec_cmd("desktopctl capture ocr"),
    { description = "Extract text (OCR) from screenshot" }
)

-- File sharing
hl.bind("SUPER + CTRL + S", hl.dsp.exec_cmd("desktopctl menu share"), { description = "Share" })

-- Transcoding
hl.bind("SUPER + CTRL + PERIOD", hl.dsp.exec_cmd("desktopctl transcode"), { description = "Transcode" })

-- Reminders
hl.bind("SUPER + CTRL + R", hl.dsp.exec_cmd("desktopctl menu reminder-set"), { description = "Set reminder" })
hl.bind("SUPER + CTRL + ALT + R", hl.dsp.exec_cmd("desktopctl reminder show"), { description = "Show reminders" })
hl.bind("SUPER + SHIFT + CTRL + R", hl.dsp.exec_cmd("desktopctl reminder clear"), { description = "Clear reminders" })

-- Waybar-less information
hl.bind(
    "SUPER + CTRL + ALT + T",
    hl.dsp.exec_cmd('notify-send -u low "    $(date +"%A %H:%M  ·  %d %B %Y  ·  Week %V")"'),
    { description = "Show time" }
)
hl.bind(
    "SUPER + CTRL + ALT + B",
    hl.dsp.exec_cmd('notify-send -u low "$(desktopctl battery)"'),
    { description = "Show battery remaining" }
)
hl.bind(
    "SUPER + CTRL + ALT + W",
    hl.dsp.exec_cmd('notify-send -u low "$(desktopctl weather)"'),
    { description = "Show weather" }
)

-- Control panels
hl.bind("SUPER + CTRL + A", hl.dsp.exec_cmd("desktopctl launch audio"), { description = "Audio controls" })
hl.bind("SUPER + CTRL + B", hl.dsp.exec_cmd("desktopctl launch bluetooth"), { description = "Bluetooth controls" })
hl.bind("SUPER + CTRL + W", hl.dsp.exec_cmd("desktopctl launch wifi"), { description = "Wifi controls" })
hl.bind("SUPER + CTRL + T", hl.dsp.exec_cmd("desktopctl launch tui btop"), { description = "Activity" })

-- Dictation
hl.bind("SUPER + CTRL + X", hl.dsp.exec_cmd("dictate live toggle"), { description = "Toggle dictation" })

hl.bind("F9", hl.dsp.exec_cmd("dictate live start"), { description = "Start dictation (push-to-talk)" })
hl.bind("F9", hl.dsp.exec_cmd("dictate live stop"), { description = "Stop dictation (push-to-talk)", release = true })

-- Zoom
hl.bind("SUPER + CTRL + Z", function()
    hl.config({ cursor = { zoom_factor = hl.get_config("cursor.zoom_factor") + 1 } })
end, { description = "Zoom in" })
hl.bind("SUPER + CTRL + ALT + Z", function()
    hl.config({ cursor = { zoom_factor = 1 } })
end, { description = "Reset zoom" })

-- Lock system
hl.bind("SUPER + CTRL + L", hl.dsp.exec_cmd("desktopctl lock"), { description = "Lock system" })
