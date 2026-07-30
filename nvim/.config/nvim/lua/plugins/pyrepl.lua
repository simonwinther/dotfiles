return {
  {
    "dangooddd/pyrepl.nvim",

    dependencies = {
      "nvim-treesitter/nvim-treesitter",
    },

    opts = {
      -- Opens the REPL beside the code rather than underneath it
      split_horizontal = false,
      split_ratio = 0.45,

      -- Ghostty-compatible image rendering
      image_provider = "placeholders",

      -- Python cells are separated with: # %%
      cell_pattern = "^# %%%%.*$",

      python_path = "python",
      preferred_kernel = "python3",

      -- Automatically offer to convert .ipynb files
      jupytext_hook = true,
    },

    config = function(_, opts)
      local pyrepl = require("pyrepl")
      pyrepl.setup(opts)

      -- REPL
      vim.keymap.set("n", "<leader>jo", pyrepl.open_repl, {
        desc = "Open Python REPL",
      })

      vim.keymap.set("n", "<leader>jt", pyrepl.toggle_repl, {
        desc = "Toggle Python REPL",
      })

      vim.keymap.set("n", "<leader>jc", pyrepl.close_repl, {
        desc = "Close Python REPL",
      })

      vim.keymap.set({ "n", "t" }, "<C-j>", pyrepl.toggle_repl_focus, {
        desc = "Toggle REPL focus",
      })

      -- Execution
      vim.keymap.set("n", "<leader>jl", pyrepl.send_cell, {
        desc = "Run Python cell",
      })

      vim.keymap.set("v", "<leader>jv", pyrepl.send_visual, {
        desc = "Run selected Python",
      })

      vim.keymap.set("n", "<leader>jb", pyrepl.send_buffer, {
        desc = "Run entire Python file",
      })

      -- Cell navigation
      vim.keymap.set("n", "]j", pyrepl.step_cell_forward, {
        desc = "Next Python cell",
      })

      vim.keymap.set("n", "[j", pyrepl.step_cell_backward, {
        desc = "Previous Python cell",
      })

      vim.keymap.set("n", "<leader>ji", pyrepl.open_image_history, {
        desc = "Python image history",
      })
    end,
  },
}
