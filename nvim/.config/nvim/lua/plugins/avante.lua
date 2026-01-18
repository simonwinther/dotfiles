-- FIX: Right now it doesn't show DIFF it just applies the changes directly to the buffer, dangerous as hell.
return {
  {
    "yetone/avante.nvim",
    build = "make",
    enabled = false,
    lazy = true,
    opts = {
      provider = "copilot",
      selection = {
        hint_display = "none",
      },
      behaviour = {
        auto_set_keymaps = true,
      },
    },
    cmd = {
      "AvanteAsk",
      "AvanteBuild",
      "AvanteChat",
      "AvanteClear",
      "AvanteEdit",
      "AvanteFocus",
      "AvanteHistory",
      "AvanteModels",
      "AvanteRefresh",
      "AvanteShowRepoMap",
      "AvanteStop",
      "AvanteSwitchProvider",
      "AvanteToggle",
    },
    keys = {
      { "<leader>aa", "<cmd>AvanteAsk<CR>", desc = "Ask Avante" },
      { "<leader>ac", "<cmd>AvanteChat<CR>", desc = "Chat with Avante" },
      { "<leader>ae", "<cmd>AvanteEdit<CR>", desc = "Edit Avante" },
      { "<leader>af", "<cmd>AvanteFocus<CR>", desc = "Focus Avante" },
      { "<leader>ah", "<cmd>AvanteHistory<CR>", desc = "Avante History" },
      { "<leader>am", "<cmd>AvanteModels<CR>", desc = "Select Avante Model" },
      { "<leader>an", "<cmd>AvanteChatNew<CR>", desc = "New Avante Chat" },
      { "<leader>ap", "<cmd>AvanteSwitchProvider<CR>", desc = "Switch Avante Provider" },
      { "<leader>ar", "<cmd>AvanteRefresh<CR>", desc = "Refresh Avante" },
      { "<leader>as", "<cmd>AvanteStop<CR>", desc = "Stop Avante" },
      { "<leader>at", "<cmd>AvanteToggle<CR>", desc = "Toggle Avante" },
    },
  },
  { "Kaiser-Yang/blink-cmp-avante", lazy = true },
}
