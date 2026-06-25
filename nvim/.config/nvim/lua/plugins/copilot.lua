return {
  {
    "zbirenbaum/copilot.lua",
    cmd = "Copilot",
    build = ":Copilot auth",
    event = "InsertEnter",
    -- dependencies = {
    --   {
    --     "copilotlsp-nvim/copilot-lsp",
    --     init = function()
    --       vim.g.copilot_nes_debounce = 500
    --     end,
    --   },
    -- },
    opts = {
      suggestion = {
        enabled = true,
        auto_trigger = false,
        trigger_on_accept = false,
        keymap = {
          -- The blink <Tab> keymap accepts Copilot suggestions itself, so
          -- disable Copilot's own <Tab> map to avoid two handlers on one key.
          accept = false,
          accept_word = "<M-l>",
          next = "<M-]>",
          prev = "<M-[>",
          dismiss = "<M-_>",
        },
      },
      -- nes = {
      --   enabled = true,
      --   auto_trigger = true,
      --   keymap = {
      --     accept = "<C-M-l>",
      --     dismiss = "<C-M-_>",
      --   },
      -- },
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
