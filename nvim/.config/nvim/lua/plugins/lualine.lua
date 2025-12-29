return {
  "nvim-lualine/lualine.nvim",
  event = "VeryLazy",
  opts = function(_, opts)
    --
    local mode_icons = {
      n = "",
      i = "",
      v = "",
      V = "",
      ["\22"] = "",
      c = "",
      t = "",
      R = "",
      s = "",
    }
    -- Ensure the section table exists
    opts.sections = opts.sections or {}
    opts.sections.lualine_a = {
      {
        "mode",
        fmt = function(str)
          local mode_code = vim.fn.mode()
          local icon = mode_icons[mode_code] or " "
          return icon .. " " .. str:sub(1, 1)
        end,
        color = function()
          local mode_color = {
            n = "#1793d1", -- Arch Linux Blue
            i = "#98c379", -- Green
            v = "#c678dd", -- Purple
            ["\22"] = "#c678dd",
            V = "#c678dd",
            c = "#e5c07b", -- Yellow
            R = "#e06c75", -- Red
          }
          local mode = vim.fn.mode()
          return { bg = mode_color[mode] or "#1793d1", fg = "#ffffff", gui = "bold" }
        end,
        padding = { right = 1, left = 1 },
      },
    }
    opts.sections.lualine_x = opts.sections.lualine_x or {}

    -- Add Search Count (The [2/13] indicator)
    -- I added this because with git blame it’s sometimes hard to see, but I want git blame on the current line = true.)
    table.insert(opts.sections.lualine_x, 1, {
      "searchcount",
      maxcount = 999,
      timeout = 500,
      icon = "",
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
