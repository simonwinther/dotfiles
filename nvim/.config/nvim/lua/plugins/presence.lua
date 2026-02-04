-- https://github.com/flathub/com.discordapp.Discord/wiki/Rich-Precense-(discord-rpc)
-- Run `Unsandboxed applications` section (Not `flatpak applications`, even though I have Discord in Flatpak)
return {
  "andweeb/presence.nvim",
  lazy = false,
  config = function()
    require("presence").setup({
      auto_update = true,
      main_image = "file",
      show_time = true,
      debounce_timeout = 15,
      buttons = true,

      editing_text = "Editing %s",
      reading_text = "Reading %s",
      workspace_text = "In %s",
      git_commit_text = "Writing a commit",

      blacklist = {
        "%.env",
        "node_modules",
        "/tmp",
        "private",
      },
    })
  end,
}
