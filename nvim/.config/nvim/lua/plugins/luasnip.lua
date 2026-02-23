return {
  {
    "L3MON4D3/LuaSnip",
    event = "InsertEnter",
    dependencies = {
      "rafamadriz/friendly-snippets",
      config = function()
        require("luasnip.loaders.from_vscode").lazy_load()
        require("luasnip.loaders.from_lua").lazy_load({ paths = { vim.fn.stdpath("config") .. "/snippets" } })

        -- Hot Reload on Save
        vim.api.nvim_create_autocmd("BufWritePost", {
          pattern = "*/snippets/**/*.lua",
          callback = function()
            require("luasnip.loaders.from_lua").load({ paths = { vim.fn.stdpath("config") .. "/snippets" } })
            print("Snippets reloaded successfully!")
          end,
        })
      end,
    },

    opts = {
      history = true,
      update_events = "TextChanged,TextChangedI",
      enable_autosnippets = true,
      -- delete_check_events = "TextChanged",
    },

    keys = {
      {
        "<C-K>",
        function()
          local ls = require("luasnip")
          if ls.expand_or_jumpable() then
            ls.expand_or_jump()
          end
        end,
        mode = { "i", "s" },
        silent = true,
      },

      -- 2. JUMP BACKWARD
      {
        "<C-J>",
        function()
          local ls = require("luasnip")
          if ls.jumpable(-1) then
            ls.jump(-1)
          end
        end,
        mode = { "i", "s" },
        silent = true,
      },

      -- 3. CHANGE CHOICE (For snippets with multiple options)
      {
        "<C-E>",
        function()
          local ls = require("luasnip")
          if ls.choice_active() then
            ls.change_choice(1)
          end
        end,
        mode = { "i", "s" },
        silent = true,
      },
    },
  },
}
