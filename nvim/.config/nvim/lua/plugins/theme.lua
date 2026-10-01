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
        local config_home = vim.env.XDG_CONFIG_HOME or (vim.env.HOME .. "/.config")
        local light = vim.uv.fs_stat(config_home .. "/omarchy/current/theme/light.mode") ~= nil
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

      vim.api.nvim_create_autocmd({ "VimEnter", "FocusGained", "VimResume" }, {
        group = vim.api.nvim_create_augroup("UserSystemAppearance", { clear = true }),
        callback = sync_appearance,
        nested = true,
        desc = "Follow Omarchy light/dark appearance",
      })
    end,
  },
}
