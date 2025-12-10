-- lua/plugins/ai_snips_all.lua
-- One big file: LuaSnip + friendly-snippets, nvim-cmp (optional) with cmp_luasnip,
-- Blink (optional) with LuaSnip preset + Copilot provider, Copilot core + ai_accept + lualine status.

return {
  ---------------------------------------------------------------------------
  -- LuaSnip + friendly-snippets
  ---------------------------------------------------------------------------
  {
    "L3MON4D3/LuaSnip",
    lazy = true,
    build = (not LazyVim.is_win())
        and "echo 'NOTE: jsregexp is optional, so not a big deal if it fails to build'; make install_jsregexp"
      or nil,
    dependencies = {
      {
        "rafamadriz/friendly-snippets",
        config = function()
          -- Lua-based snippets (~/.config/nvim/lua/snips/*.lua)
          pcall(function()
            require("luasnip.loaders.from_lua").lazy_load({
              paths = { vim.fn.stdpath("config") .. "/snippets" },
            })
          end)
        end,
      },
    },
    opts = {
      history = true,
      delete_check_events = "TextChanged",
    },
  },

  -- Add snippet_forward / snippet_stop actions to LazyVim.cmp
  {
    "L3MON4D3/LuaSnip",
    opts = function()
      LazyVim.cmp.actions.snippet_forward = function()
        if require("luasnip").jumpable(1) then
          vim.schedule(function()
            require("luasnip").jump(1)
          end)
          return true
        end
      end
      LazyVim.cmp.actions.snippet_stop = function()
        if require("luasnip").expand_or_jumpable() then
          require("luasnip").unlink_current()
          return true
        end
      end
    end,
  },

  ---------------------------------------------------------------------------
  -- nvim-cmp (OPTIONAL) + cmp_luasnip source
  ---------------------------------------------------------------------------
  {
    "hrsh7th/nvim-cmp",
    optional = true,
    event = "InsertEnter",
    dependencies = {
      "L3MON4D3/LuaSnip",
      { "saadparwaiz1/cmp_luasnip", event = "InsertEnter", dependencies = { "hrsh7th/nvim-cmp" } },
      "hrsh7th/cmp-nvim-lsp",
      "hrsh7th/cmp-buffer",
      "hrsh7th/cmp-path",
    },
    opts = function(_, opts)
      local cmp = require("cmp")
      opts = opts or {}
      opts.snippet = {
        expand = function(args)
          require("luasnip").lsp_expand(args.body)
        end,
      }
      opts.mapping = opts.mapping
        or cmp.mapping.preset.insert({
          ["<CR>"] = cmp.mapping.confirm({ select = false }),
        })
      opts.sources = opts.sources or {}
      table.insert(opts.sources, { name = "nvim_lsp" })
      table.insert(opts.sources, { name = "luasnip" })
      table.insert(opts.sources, { name = "buffer" })
      table.insert(opts.sources, { name = "path" })
      return opts
    end,
    keys = {
      {
        "<tab>",
        function()
          require("luasnip").jump(1)
        end,
        mode = "s",
      },
      {
        "<s-tab>",
        function()
          require("luasnip").jump(-1)
        end,
        mode = { "i", "s" },
      },
    },
  },

  ---------------------------------------------------------------------------
  -- Blink (OPTIONAL): LuaSnip preset + Copilot provider (blink-cmp-copilot)
  ---------------------------------------------------------------------------
  {
    "saghen/blink.cmp",
    optional = true,
    dependencies = {
      "L3MON4D3/LuaSnip",
      "giuxtaposition/blink-cmp-copilot",
    },
    opts = function(_, opts)
      opts = opts or {}

      -- Use LuaSnip with Blink
      opts.snippets = opts.snippets or {}
      opts.snippets.preset = "luasnip"

      -- Providers
      opts.sources = opts.sources or {}
      opts.sources.providers = vim.tbl_deep_extend("force", opts.sources.providers or {}, {
        copilot = {
          name = "copilot",
          module = "blink-cmp-copilot",
          async = true,
          score_offset = 100,
          -- Some adapters set a custom kind; some don't. Either way we'll icon-patch below.
          kind = "Copilot",
        },
      })

      -- Enable sources in popup
      local want = { "lsp", "path", "buffer", "snippets", "copilot" }
      local cur = vim.list_extend({}, opts.sources.default or {})
      local seen = {}
      for _, s in ipairs(cur) do
        seen[s] = true
      end
      for _, s in ipairs(want) do
        if not seen[s] then
          table.insert(cur, s)
        end
      end
      opts.sources.default = cur

      -- Appearance: icon map + alignment
      opts.appearance = vim.tbl_deep_extend("force", opts.appearance or {}, {
        -- Helps alignment with Nerd Font
        nerd_font_variant = "mono",
        -- If your theme lacks Blink highlights, you can turn this on:
        -- use_nvim_cmp_as_default = true,
        kind_icons = vim.tbl_extend("force", {
          Copilot = "", -- Octocat
          Snippet = "",
        }, (opts.appearance and opts.appearance.kind_icons) or {}),
      })

      -- Menu render: show source name & force copilot icon if needed
      opts.completion = opts.completion or {}
      opts.completion.menu = opts.completion.menu or {}
      opts.completion.menu.draw = opts.completion.menu.draw or {}

      -- Add a rightmost column with [source]
      opts.completion.menu.draw.columns = opts.completion.menu.draw.columns
        or {
          { "kind_icon" },
          { "label", "label_description", gap = 1 },
          { "source_name" },
        }

      -- Custom components
      opts.completion.menu.draw.components = vim.tbl_deep_extend("force", opts.completion.menu.draw.components or {}, {
        -- Make sure Copilot items always show the octocat, even if kind wasn't set
        kind_icon = {
          text = function(ctx)
            if ctx.source_id == "copilot" then
              return ""
            end
            return ctx.kind_icon
          end,
        },
        -- Show [copilot]/[lsp]/[buffer] etc.
        source_name = {
          width = { max = 32 },
          highlight = "BlinkCmpSource",
          text = function(ctx)
            return "[" .. tostring(ctx.source_name or ctx.source_id or "?") .. "]"
          end,
        },
      })

      -- IMPORTANT: turn off Blink's own ghost text to avoid clashing with Copilot's inline ghost
      opts.completion.ghost_text = { enabled = false }

      ------------------------------------------------------------------
      -- Keymaps: keep default preset, just make <CR> accept completion
      ------------------------------------------------------------------
      opts.keymap = opts.keymap or {}
      opts.keymap.preset = opts.keymap.preset or "default"

      -- When menu is open: <CR> accepts; otherwise falls back to normal Enter
      opts.keymap["<Tab>"] = { "accept", "fallback" }

      return opts
    end,
  },
  ---------------------------------------------------------------------------
  -- Copilot core + ai_accept + disable LSP "copilot" + lualine status (opt)
  ---------------------------------------------------------------------------
  {
    "zbirenbaum/copilot.lua",
    version = "*",
    cmd = "Copilot",
    build = ":Copilot auth",
    event = "InsertEnter",
    opts = {
      suggestion = {
        enabled = true,
        auto_trigger = false, -- don't auto trigger, use keymaps to go through
        hide_during_completion = false,
        keymap = {
          accept = false,
          accept_word = "<C-l>",
          next = "<M-]>",
          prev = "<M-[>",
          dismiss = "<C-_>",
        },
      },
      -- 👇 enable panel to view multiple suggestions
      panel = {
        enabled = true,
        auto_refresh = true,
        keymap = {
          jump_prev = "[[",
          jump_next = "]]",
          accept = "<CR>",
          refresh = "gr",
          open = "<M-CR>", -- you can change this
        },
        layout = {
          position = "bottom", -- or "right"
          ratio = 0.4,
        },
      },
      filetypes = { markdown = true, help = true },
    },
  },

  {
    "zbirenbaum/copilot.lua",
    opts = function()
      LazyVim.cmp.actions.ai_accept = function()
        local ok, sug = pcall(require, "copilot.suggestion")
        if ok and sug.is_visible() then
          LazyVim.create_undo()
          sug.accept()
          return true
        end
      end
    end,
  },

  -- Ensure lspconfig does NOT start a "copilot" LSP
  {
    "neovim/nvim-lspconfig",
    optional = true,
    opts = {
      servers = { copilot = { enabled = false } },
    },
  },
}
