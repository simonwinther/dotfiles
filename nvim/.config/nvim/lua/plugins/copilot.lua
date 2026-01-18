return {
  {
    "zbirenbaum/copilot.lua",
    cmd = "Copilot",
    build = ":Copilot auth",
    event = "InsertEnter",
    opts = {
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
