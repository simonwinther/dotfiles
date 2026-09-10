-- See https://wiki.hyprland.org/Configuring/Monitors/
-- List current monitors and resolutions possible: hyprctl monitors
-- See the hl.monitor tables below for output, mode, position, and scale.

-- Optimized for retina-class 2x displays, like 13" 2.8K, 27" 5K, 32" 6K.
hl.env("GDK_SCALE", "2")
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = "auto" })

-- Good compromise for 27" or 32" 4K monitors (but fractional!)
-- hl.env("GDK_SCALE", "1.75")
-- hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1.6 })

-- Straight 1x setup for low-resolution displays like 1080p or 1440p
-- hl.env("GDK_SCALE", "1")
-- hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })

-- Example for Framework 13 w/ 6K XDR Apple display
-- hl.monitor({ output = "DP-5", mode = "6016x3384@60", position = "auto", scale = 2 })
-- hl.monitor({ output = "eDP-1", mode = "2880x1920@120", position = "auto", scale = 2 })

-- Setup when working in common room (Dorm)
-- This is why Nix would be nice, so I don't do this hardcoded shit, but whatever
-- hl.monitor({ output = "HDMI-A-1", mode = "1920x1080@60", position = "0x0", scale = 1 })
-- hl.monitor({ output = "eDP-1", mode = "1920x1080@60.02", position = "0x1080", scale = 1 })

-- Desktop monitors
hl.monitor({ output = "HDMI-A-1", mode = "1920x1080@100", position = "0x0", scale = 1 })
hl.monitor({ output = "DP-2", mode = "1920x1080@165", position = "1920x0", scale = 1 })
hl.monitor({ output = "DVI-D-1", mode = "1920x1080@144", position = "3840x0", scale = 1, transform = 3 })

-- Laptop
hl.monitor({ output = "eDP-1", mode = "1920x1080@60.02", position = "0x0", scale = 1.2 })

-- AI Pioneer (top-to-bottom)
-- hl.monitor({ output = "HDMI-A-1", mode = "2560x1440@59.95", position = "0x0", scale = 1 })
-- hl.monitor({ output = "eDP-1", mode = "1920x1080@60.02", position = "480x1440", scale = 1.2 })

-- # Left-to-right: laptop on the left, external monitor on the right
-- hl.monitor({ output = "eDP-1", mode = "1920x1080@60.02", position = "0x270", scale = 1.2 })
-- hl.monitor({ output = "HDMI-A-1", mode = "2560x1440@59.95", position = "1600x0", scale = 1 })

-- bottom-to-top: laptop below, external monitor above
-- hl.monitor({ output = "eDP-1", mode = "1920x1080@60.02", position = "160x1080", scale = 1.2 })

-- Left-to-right: laptop on the left, external monitor on the right
-- hl.monitor({ output = "eDP-1", mode = "1920x1080@60.02", position = "0x270", scale = 1.2 })
-- hl.monitor({ output = "HDMI-A-1", mode = "2560x1440@59.95", position = "1600x0", scale = 1 })

-- Right-to-left: external monitor on the left, laptop on the right
-- hl.monitor({ output = "HDMI-A-1", mode = "2560x1440@59.95", position = "0x0", scale = 1 })
-- hl.monitor({ output = "eDP-1", mode = "1920x1080@60.02", position = "2560x270", scale = 1.2 })
