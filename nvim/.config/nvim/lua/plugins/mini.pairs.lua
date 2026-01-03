return {
  "nvim-mini/mini.pairs",
  opts = function(_, opts)
    opts = opts or {}
    -- Stops using mini.pairs in command and terminal modes
    opts.modes = vim.tbl_extend("force", opts.modes or {}, {
      insert = true,
      command = false,
      terminal = false,
    })
    return opts
  end,
}
