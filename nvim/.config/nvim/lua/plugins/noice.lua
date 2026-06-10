return {
  "folke/noice.nvim",
  opts = function(_, opts)
    local function trim_trailing_blank_lines(message)
      local lines = message._lines
      while #lines > 1 and not lines[#lines]:content():find("%S") do
        table.remove(lines)
      end
    end

    opts.lsp = opts.lsp or {}
    opts.lsp.signature = vim.tbl_deep_extend("force", opts.lsp.signature or {}, {
      enabled = true,
      auto_open = {
        enabled = true,
        trigger = true,
        luasnip = true,
        snipppets = true,
        throttle = 25,
      },
      view = "signature_popup",
    })

    opts.views = opts.views or {}
    opts.views.hover = vim.tbl_deep_extend("force", opts.views.hover or {}, {
      size = {
        width = "auto",
        height = "auto",
        max_width = 100,
        max_height = 12,
      },
      border = {
        style = "rounded",
        padding = { 0, 1 },
      },
      position = { row = 2, col = 2 },
      scrollbar = false,
    })

    opts.views.signature_popup = {
      view = "hover",
      anchor = "NW",
      enter = false,
      focusable = false,
      scrollbar = false,
      position = {
        row = 2,
        col = 0,
      },
      size = {
        width = "auto",
        height = "auto",
        max_width = 100,
        max_height = 12,
      },
      border = {
        style = "rounded",
        padding = { 0, 1 },
      },
      win_options = {
        wrap = false,
        linebreak = false,
        cursorline = false,
      },
    }

    local signature = require("noice.lsp.signature")
    if not signature._dotfiles_trim_empty_lines then
      local format = signature.format
      signature.format = function(self, ...)
        format(self, ...)
        trim_trailing_blank_lines(self.message)
      end
      signature._dotfiles_trim_empty_lines = true
    end

    return opts
  end,
}
