-- ~/.config/nvim/lua/plugins/markdown.lua
return {
  {
    "OXY2DEV/markview.nvim",
    ft = { "markdown" },
    opts = {
      preview = { icon_provider = "internal" }, -- optional
    },
    config = function(_, opts)
      local presets = require("markview.presets") -- safe here
      opts.markdown = opts.markdown or {}
      opts.markdown.headings = presets.headings.slanted -- pick a preset
      require("markview").setup(opts)
    end,
  },
}
