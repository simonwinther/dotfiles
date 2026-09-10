-- Extra autostart processes
hl.on("hyprland.start", function()
    hl.exec_cmd("uwsm app -- ~/.local/bin/workspaces-slider")
    hl.exec_cmd("uwsm app -- waybar -c ~/.config/waybar/config-minimal.jsonc -s ~/.config/waybar/style-minimal.css")
end)
