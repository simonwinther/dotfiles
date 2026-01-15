return {
  "nvim-lualine/lualine.nvim",
  event = "VeryLazy",
  opts = function(_, opts)
    -- Define mode abbreviations
    local mode_map = {
      NORMAL = "N",
      INSERT = "I",
      VISUAL = "V",
      ["V-LINE"] = "VL",
      ["V-BLOCK"] = "VB",
      REPLACE = "R",
      COMMAND = "C",
      TERMINAL = "T",
      SELECT = "S",
    }
    -- Define mode icons
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
      S = "",
    }
    -- Define mode colors
    local mode_color = {
      n = "#1793d1",
      i = "#98c379",
      v = "#c678dd",
      s = "#c678dd",
      S = "#c678dd",
      ["\22"] = "#c678dd",
      V = "#c678dd",
      c = "#e5c07b",
      R = "#e06c75",
      t = "#e06c75",
    }
    -- Ensure the section table exists
    opts.sections = opts.sections or {}
    opts.sections.lualine_a = {
      {
        "mode",
        fmt = function(str)
          local mode_code = vim.fn.mode()
          local icon = mode_icons[mode_code] or " "
          return icon .. " " .. (mode_map[str] or str)
        end,
        color = function()
          local mode = vim.fn.mode()
          return { bg = mode_color[mode] or "#1793d1", fg = "#ffffff", gui = "bold" }
        end,
        padding = { right = 1, left = 1 },
      },
    }
    opts.sections.lualine_x = opts.sections.lualine_x or {}

    -- Add Search Count (The [2/13] indicator)
    -- I added this because with git blame it’s sometimes hard to see, but I want git blame on the current line = true.)
    table.insert(opts.sections.lualine_y, 1, {
      "searchcount",
      maxcount = 999,
      timeout = 500,
    })

    -- I overwrite lualine_z (is timer by default) to show filesize
    opts.sections.lualine_z = {
      {
        "filesize",
        icon = "",
        padding = { left = 1, right = 1 },
      },
    }
  end,
}
