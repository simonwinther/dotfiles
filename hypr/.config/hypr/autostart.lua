-- Extra autostart processes
hl.on("hyprland.start", function()
    hl.exec_cmd("~/.local/bin/file-picker --warm")
    hl.exec_cmd("~/.local/bin/file-picker --warm --pdf")
    hl.exec_cmd("~/.local/bin/file-picker --warm --recent")
    hl.exec_cmd("uwsm app -- ~/.local/bin/workspaces-slider-launch")
    hl.exec_cmd("uwsm app -- waybar -c ~/.config/waybar/config-minimal.jsonc -s ~/.config/waybar/style-minimal.css")
end)
