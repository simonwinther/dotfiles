return {
  {
    "L3MON4D3/LuaSnip",
    event = "InsertEnter",
    dependencies = {
      {
        "rafamadriz/friendly-snippets",
        config = function()
          require("luasnip.loaders.from_vscode").lazy_load()
          require("luasnip.loaders.from_vscode").lazy_load({ paths = { vim.fn.stdpath("config") .. "/snippets" } })
        end,
      },
    },
    opts = function()
      local ls = require("luasnip")
      LazyVim.cmp.actions.snippet_forward = function()
        if ls.jumpable(1) then
          ls.jump(1)
          return true
        end
      end
      LazyVim.cmp.actions.snippet_stop = function()
        if ls.expand_or_jumpable() then
          ls.unlink_current()
          return true
        end
      end

      return {
        history = true,
        delete_check_events = "TextChanged",
      }
    end,
  },
}
