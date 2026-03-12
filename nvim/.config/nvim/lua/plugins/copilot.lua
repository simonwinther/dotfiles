return {
  {
    "zbirenbaum/copilot.lua",
    cmd = "Copilot",
    build = ":Copilot auth",
    event = "InsertEnter",
    dependencies = {
      {
        "copilotlsp-nvim/copilot-lsp",
        init = function()
          vim.g.copilot_nes_debounce = 500
        end,
      },
    },
    opts = {
      suggestion = {
        enabled = true,
        auto_trigger = false,
        keymap = {
          accept = "<Tab>",
          accept_word = "<C-l>",
          next = "<M-]>",
          prev = "<M-[>",
          dismiss = "<M-_>",
        },
      },
      nes = {
        enabled = true,
        auto_trigger = true,
        keymap = {
          accept = "<C-M-l>",
          dismiss = "<C-M-_>",
        },
      },
      filetypes = { markdown = true, help = true },
    },
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
