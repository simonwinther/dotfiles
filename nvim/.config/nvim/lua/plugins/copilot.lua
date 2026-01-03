return {
  {
    "zbirenbaum/copilot.lua",
    cmd = "Copilot",
    event = "InsertEnter",
    build = ":Copilot auth",
    opts = function()
      LazyVim.cmp.actions.ai_accept = function()
        local suggestion = require("copilot.suggestion")
        if suggestion.is_visible() then
          LazyVim.create_undo()
          suggestion.accept()
          return true
        end
      end

      return {
        suggestion = {
          enabled = true,
          auto_trigger = false,
          keymap = {
            accept = false,
            accept_word = "<C-l>",
            next = "<M-]>",
            prev = "<M-[>",
            dismiss = "<C-_>",
          },
        },
        panel = {
          enabled = true,
          auto_refresh = true,
          keymap = {
            open = "<M-CR>",
          },
          layout = { position = "bottom", ratio = 0.4 },
        },
        filetypes = { markdown = true, help = true },
      }
    end,
  },
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        copilot = { enabled = false },
      },
    },
  },
}
