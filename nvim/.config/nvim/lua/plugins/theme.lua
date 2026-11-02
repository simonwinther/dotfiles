return {
  { "folke/tokyonight.nvim", enabled = false },
  {
    "catppuccin/nvim",
    name = "catppuccin",
    lazy = false,
    priority = 1000,
    build = ":CatppuccinCompile",
    opts = {
      flavour = "auto",
      background = { light = "latte", dark = "mocha" },
      transparent_background = true, -- Set to true if you want your Ghostty terminal background to show through
      styles = {
        comments = { "italic" },
        keywords = { "bold" },
      },
    },
    config = function(_, opts)
      local function sync_appearance()
        local light = vim.uv.fs_stat((vim.env.XDG_STATE_HOME or (vim.env.HOME .. "/.local/state")) .. "/desktop/theme/light.mode") ~= nil
        local background = light and "light" or "dark"
        if vim.o.background ~= background then
          vim.o.background = background
          -- Reapply ColorScheme hooks for custom highlights and the statusline.
          if package.loaded["catppuccin"] then
            vim.cmd.colorscheme("catppuccin")
          end
        end
      end

      sync_appearance()
      require("catppuccin").setup(opts)
      vim.cmd.colorscheme("catppuccin")

      local group = vim.api.nvim_create_augroup("UserSystemAppearance", { clear = true })
      vim.api.nvim_create_autocmd({ "VimEnter", "FocusGained", "VimResume" }, {
        group = group,
        callback = sync_appearance,
        nested = true,
        desc = "Follow Desktop light/dark appearance",
      })
      vim.api.nvim_create_autocmd("Signal", {
        group = group,
        pattern = "SIGUSR1",
        callback = sync_appearance,
        nested = true,
        desc = "Apply Desktop appearance immediately after a theme switch",
      })
    end,
  },
}
