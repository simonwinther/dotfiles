return {
  "nvim-lualine/lualine.nvim",
  event = "VeryLazy",
  opts = function(_, opts)
    -- Ensure the section table exists
    opts.sections = opts.sections or {}
    opts.sections.lualine_a = {
      {
        "mode",
        fmt = function(str)
          local mode_map = {
            ["NORMAL"] = "N",
            ["INSERT"] = "I",
            ["VISUAL"] = "V",
            ["V-LINE"] = "VL",
            ["V-BLOCK"] = "VB",
            ["REPLACE"] = "R",
            ["COMMAND"] = "C",
            ["TERMINAL"] = "T",
          }
          return mode_map[str] or "unknown"
        end,
      },
    }
    opts.sections.lualine_x = opts.sections.lualine_x or {}

    -- Add Search Count (The [2/13] indicator)
    -- I added this because with git blame it’s sometimes hard to see, but I want git blame on the current line = true.)
    table.insert(opts.sections.lualine_x, 1, {
      "searchcount",
      maxcount = 999,
      timeout = 500,
    })

    -- Add Copilot Status
    table.insert(
      opts.sections.lualine_x,
      2,
      LazyVim.lualine.status(LazyVim.config.icons.kinds.Copilot, function()
        local clients = package.loaded["copilot"] and vim.lsp.get_clients({ name = "copilot", bufnr = 0 }) or {}
        if #clients > 0 then
          local status = require("copilot.status").data.status
          return (status == "InProgress" and "pending") or (status == "Warning" and "error") or "ok"
        end
      end)
    )
  end,
}
