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
        -- Disable ghost text (prevents clash with Copilot's ghost text)
        ghost_text = { enabled = false },
        menu = {
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

      -- Keymaps
      keymap = {
        preset = "default",
        ["<Tab>"] = { "accept", "fallback" },
        -- ["<C-j>"] = { "select_next", "fallback" },
        -- ["<C-k>"] = { "select_prev", "fallback" },
      },
    },
  },
}
