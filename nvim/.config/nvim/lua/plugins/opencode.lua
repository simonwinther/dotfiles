-- plugins/opencode.lua
-- Loads opencode-tmux from ~/dev/opencode-tmux

return {
  dir = "~/dev/opencode-tmux",
  name = "opencode-tmux",

  keys = {
    {
      "<leader>oo",
      function()
        require("opencode-tmux").tmux_toggle()
      end,
      mode = { "n", "v" },
      desc = "Toggle OpenCode pane",
    },
    {
      "go",
      function()
        require("opencode-tmux").send()
      end,
      mode = { "n", "v" },
      desc = "Send to OpenCode",
    },
    {
      "<leader>oB",
      function()
        require("opencode-tmux").send_buffer()
      end,
      desc = "Send buffer with prompt",
    },
    {
      "<leader>op",
      function()
        require("opencode-tmux").select_prompt()
      end,
      mode = { "n", "v" },
      desc = "Pick a prompt",
    },
    {
      "<leader>oa",
      function()
        require("opencode-tmux").ask({ submit = true })
      end,
      mode = { "n", "v" },
      desc = "Ask OpenCode",
    },
    {
      "<leader>os",
      function()
        require("opencode-tmux").submit_prompt()
      end,
      desc = "Submit OpenCode prompt",
    },
    {
      "<leader>oc",
      function()
        require("opencode-tmux").clear_prompt()
      end,
      desc = "Clear OpenCode prompt",
    },
  },

  config = function()
    require("opencode-tmux").setup({
      port = 4096,
      split = "h",
      size = 40,
      compact_context = true,
    })
  end,
}
