local function desktop_palette(path, background)
  local file = io.open(path, "r")
  if not file then
    return {}
  end
  local colors = {}
  for line in file:lines() do
    local key, hex = line:match('^%s*([%w_]+)%s*=%s*"(#%x%x%x%x%x%x)"')
    if key then
      colors[key] = hex
    end
  end
  file:close()

  local fg, bg = colors.foreground, colors.background
  if not (fg and bg) then
    return {}
  end
  local blend = require("catppuccin.utils.colors").blend
  local edge = background == "light" and "#ffffff" or "#000000"
  return {
    base = bg, text = fg,
    mantle = blend(edge, bg, 0.12), crust = blend(edge, bg, 0.22),
    surface0 = blend(fg, bg, 0.15), surface1 = blend(fg, bg, 0.24), surface2 = blend(fg, bg, 0.33),
    overlay0 = blend(fg, bg, 0.48), overlay1 = blend(fg, bg, 0.58), overlay2 = blend(fg, bg, 0.68),
    subtext0 = blend(fg, bg, 0.78), subtext1 = blend(fg, bg, 0.89),
    red = colors.color1, maroon = colors.color9, green = colors.color2, yellow = colors.color3,
    blue = colors.color4, mauve = colors.color5, teal = colors.color6, peach = colors.color11,
    sapphire = colors.color12, sky = colors.color14, lavender = colors.accent,
    pink = colors.color13, flamingo = colors.color9, rosewater = colors.color11,
  }
end

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
      local theme = (vim.env.XDG_STATE_HOME or (vim.env.HOME .. "/.local/state")) .. "/desktop/theme"
      local applied_palette, applied_background
      local function sync_appearance()
        local background = vim.uv.fs_stat(theme .. "/light.mode") and "light" or "dark"
        local palette = desktop_palette(theme .. "/colors.toml", background)
        if applied_background == background and vim.o.background == background and vim.deep_equal(applied_palette, palette) then
          return
        end
        vim.o.background = background
        local flavour = opts.background[background]
        local config = vim.tbl_deep_extend("force", {}, opts, { color_overrides = { [flavour] = palette } })
        require("catppuccin").setup(config)
        vim.cmd.colorscheme("catppuccin")
        applied_palette, applied_background = palette, background
      end

      sync_appearance()

      local group = vim.api.nvim_create_augroup("UserSystemAppearance", { clear = true })
      vim.api.nvim_create_autocmd({ "VimEnter", "FocusGained", "VimResume" }, {
        group = group,
        callback = sync_appearance,
        nested = true,
        desc = "Follow the active desktop palette",
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
