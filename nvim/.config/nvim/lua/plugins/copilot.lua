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
    config = function(_, opts)
      local command = require("copilot.command")
      local client = require("copilot.client")
      local enable = command.enable
      -- Re-enabling a running client can leave suggestions waiting on an old on_init.
      command.enable = function(...)
        if client.setup_pending or (client.get() and not client.is_disabled()) then
          return
        end
        return enable(...)
      end
      require("copilot").setup(opts)
    end,
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
