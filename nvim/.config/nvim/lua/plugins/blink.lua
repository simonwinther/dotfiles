local function cmdline_range_prefix_width()
  local range = vim.fn.getcmdline():match("^%s*'<%s*,%s*'>%s*")
    or vim.fn.getcmdline():match("^%s*%d+%s*,%s*%d+%s*")

  return range and vim.fn.strdisplaywidth(range) or 0
end

return {
  {
    "saghen/blink.cmp",
    dependencies = {
      "giuxtaposition/blink-cmp-copilot",
    },
    opts = {
      -- Use LuaSnip as the snippet engine
      snippets = { preset = "luasnip" },

      -- Define Sources
      sources = {
        default = { "lsp", "path", "snippets", "buffer", "copilot" },
        providers = {
          copilot = {
            name = "copilot",
            module = "blink-cmp-copilot",
            score_offset = 100,
            async = true,
            transform_items = function(_, items)
              local CompletionItemKind = require("blink.cmp.types").CompletionItemKind
              local kind_idx = CompletionItemKind.Copilot
              items = items or {}
              for _, item in ipairs(items) do
                item.kind = kind_idx
              end
              return items
            end,
          },
        },
      },

      -- Appearance & Icons
      appearance = {
        kind_icons = {
          Copilot = "",
        },
      },

      -- Menu Customization (Right column for source name)
      completion = {
        list = {
          selection = {
            preselect = true,
            auto_insert = false,
          },
        },
        -- Disable ghost text (prevents clash with Copilot's ghost text)
        ghost_text = { enabled = false },
        documentation = {
          window = {
            border = "rounded",
            max_width = 100,
            max_height = 12,
            scrollbar = false,
          },
        },
        menu = {
          border = "rounded",
          scrollbar = false,
          cmdline_position = function()
            if vim.g.ui_cmdline_pos ~= nil then
              local pos = vim.g.ui_cmdline_pos
              local ok, menu = pcall(require, "blink.cmp.completion.windows.menu")
              if ok and menu.win and menu.win:is_open() then
                pcall(vim.api.nvim_win_set_config, menu.win:get_win(), {
                  border = { "", "", "╭", "│", "╯", "─", "╰", "│" },
                })
              end

              return { pos[1] - 1, math.max(pos[2] - cmdline_range_prefix_width(), 0) }
            end

            local height = (vim.o.cmdheight == 0) and 1 or vim.o.cmdheight
            return { vim.o.lines - height, 0 }
          end,
          draw = {
            columns = { { "kind_icon" }, { "label", "label_description", gap = 1 }, { "source_name" } },
            components = {
              source_name = {
                text = function(ctx)
                  return "[" .. ctx.source_name .. "]"
                end,
                highlight = "BlinkCmpSource",
              },
            },
          },
        },
      },

      signature = {
        window = {
          border = "rounded",
          max_width = 100,
          max_height = 12,
          scrollbar = false,
        },
      },

      -- Keymaps
      keymap = {
        preset = "default",
        ["<C-k>"] = { "show_signature", "hide_signature", "fallback" },
        ["<Tab>"] = {
          function(cmp)
            local ok, suggestion = pcall(require, "copilot.suggestion")
            if ok and suggestion.is_visible() then
              suggestion.accept()
              return true
            end

            if cmp.is_menu_visible() then
              return cmp.select_and_accept()
            end
          end,
          "fallback",
        },
        ["<S-Tab>"] = {
          function()
            return vim.api.nvim_replace_termcodes("<C-d>", true, true, true)
          end,
        },
      },
    },
  },
}
