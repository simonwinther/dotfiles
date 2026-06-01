return {
  "nvim-lualine/lualine.nvim",
  event = "VeryLazy",
  opts = function(_, opts)
    local colors = {
      bg = "#1e1e2e",
      fg = "#cdd6f4",
      blue = "#89b4fa",
      green = "#a6e3a1",
      violet = "#cba6f7",
      yellow = "#f9e2af",
      red = "#f38ba8",
      peach = "#fab387",
      teal = "#94e2d5",
    }

    -- Define full mode names
    local mode_map = {
      NORMAL = "NORMAL",
      INSERT = "INSERT",
      VISUAL = "VISUAL",
      ["V-LINE"] = "V-LINE",
      ["V-BLOCK"] = "V-BLOCK",
      REPLACE = "REPLACE",
      COMMAND = "COMMAND",
      TERMINAL = "TERMINAL",
      SELECT = "SELECT",
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
      n = colors.blue,
      i = colors.green,
      v = colors.violet,
      s = colors.violet,
      S = colors.violet,
      ["\22"] = colors.violet,
      V = colors.violet,
      c = colors.yellow,
      R = colors.red,
      t = colors.peach,
    }

    local formatter_cache = {}
    local lsp_cache = {}

    local function update_formatter(bufnr)
      bufnr = bufnr or vim.api.nvim_get_current_buf()
      if not vim.api.nvim_buf_is_valid(bufnr) or not package.loaded["conform"] then
        formatter_cache[bufnr] = ""
        return
      end

      local ok, conform = pcall(require, "conform")
      if not ok then
        formatter_cache[bufnr] = ""
        return
      end

      local formatters = conform.list_formatters(bufnr)
      if #formatters == 0 then
        formatter_cache[bufnr] = ""
        return
      end

      formatter_cache[bufnr] = "󰉿 " .. formatters[1].name
    end

    local function update_lsp_clients(bufnr)
      bufnr = bufnr or vim.api.nvim_get_current_buf()
      if not vim.api.nvim_buf_is_valid(bufnr) then
        return
      end

      local clients = vim.lsp.get_clients({ bufnr = bufnr })
      if #clients == 0 then
        lsp_cache[bufnr] = ""
        return
      end

      local names = {}
      for _, client in ipairs(clients) do
        table.insert(names, client.name)
      end

      lsp_cache[bufnr] = " " .. table.concat(names, ",")
    end

    local function refresh_lualine()
      if package.loaded["lualine"] then
        require("lualine").refresh({ place = { "statusline" } })
      end
    end

    local function update_status_cache(bufnr)
      update_formatter(bufnr)
      update_lsp_clients(bufnr)
      refresh_lualine()
    end

    local function schedule_status_cache_update(bufnr)
      vim.schedule(function()
        update_status_cache(bufnr)
      end)
    end

    local group = vim.api.nvim_create_augroup("UserLualineStatusCache", { clear = true })
    vim.api.nvim_create_autocmd({ "BufEnter", "FileType" }, {
      group = group,
      callback = function(args)
        update_status_cache(args.buf)
      end,
    })

    vim.api.nvim_create_autocmd({ "LspAttach", "LspDetach" }, {
      group = group,
      callback = function(args)
        schedule_status_cache_update(args.buf)
      end,
    })

    vim.api.nvim_create_autocmd("User", {
      group = group,
      pattern = "LazyLoad",
      callback = function(args)
        local plugin = type(args.data) == "table" and args.data.name or args.data
        if plugin == "conform.nvim" then
          update_status_cache(vim.api.nvim_get_current_buf())
        end
      end,
    })

    local function attached_formatter()
      local bufnr = vim.api.nvim_get_current_buf()
      if formatter_cache[bufnr] == nil then
        update_formatter(bufnr)
      end
      return formatter_cache[bufnr] or ""
    end

    local function lsp_clients()
      local bufnr = vim.api.nvim_get_current_buf()
      if lsp_cache[bufnr] == nil then
        update_lsp_clients(bufnr)
      end
      return lsp_cache[bufnr] or ""
    end

    opts.options = opts.options or {}

    -- Angled separators like your original style
    opts.options.section_separators = {
      left = "",
      right = "",
    }

    opts.options.component_separators = {
      left = "",
      right = "",
    }

    opts.sections = opts.sections or {}

    opts.sections.lualine_a = {
      {
        "mode",
        fmt = function(str)
          local mode_code = vim.fn.mode()
          local icon = mode_icons[mode_code] or ""
          return icon .. " " .. (mode_map[str] or str)
        end,
        color = function()
          local mode = vim.fn.mode()
          return {
            bg = mode_color[mode] or colors.blue,
            fg = colors.bg,
            gui = "bold",
          }
        end,
        padding = { right = 1, left = 1 },
      },
    }

    -- Keep your existing paths/functions from LazyVim
    opts.sections.lualine_c = opts.sections.lualine_c or {}

    -- Keep LazyVim's profiler/noice status slots first so showcmd stays leftmost.
    opts.sections.lualine_x = opts.sections.lualine_x or {}
    local custom_status_index = math.min(4, #opts.sections.lualine_x + 1)

    table.insert(opts.sections.lualine_x, custom_status_index, {
      attached_formatter,
      color = { fg = colors.teal, gui = "bold" },
    })

    table.insert(opts.sections.lualine_x, custom_status_index + 1, {
      lsp_clients,
      color = { fg = colors.violet, gui = "bold" },
    })

    table.insert(opts.sections.lualine_x, custom_status_index + 2, {
      "filetype",
      icon_only = false,
      colored = true,
    })

    -- Add Search Count (The [2/13] indicator)
    -- I added this because with git blame it’s sometimes hard to see, but I want git blame on the current line = true.)
    opts.sections.lualine_y = opts.sections.lualine_y or {}
    table.insert(opts.sections.lualine_y, 1, {
      "searchcount",
      maxcount = 999,
      timeout = 500,
      color = { fg = colors.yellow, gui = "bold" },
    })

    opts.sections.lualine_z = {}
  end,
}
