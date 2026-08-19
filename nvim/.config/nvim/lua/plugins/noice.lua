return {
  "folke/noice.nvim",
  opts = function(_, opts)
    opts.lsp = opts.lsp or {}
    opts.lsp.signature = vim.tbl_deep_extend("force", opts.lsp.signature or {}, {
      -- Blink owns signature help. Noice can display a late LSP response after
      -- switching buffers, which is how Python signatures ended up over images.
      enabled = false,
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

    return opts
  end,
}
